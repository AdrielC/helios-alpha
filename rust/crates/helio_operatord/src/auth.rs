use crate::types::CommandSession;
use hmac::{Hmac, Mac};
use http::header::COOKIE;
use http::HeaderMap;
use sha2::{Digest, Sha256};
use std::fmt;
use subtle::ConstantTimeEq;
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

type HmacSha256 = Hmac<Sha256>;

const SESSION_COOKIE: &str = "helios_operator_session";

#[derive(Clone)]
pub struct CommandAuth {
    operator: String,
    token_digest: Option<[u8; 32]>,
    csrf_token: Option<String>,
    session_ttl: Duration,
}

impl fmt::Debug for CommandAuth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandAuth")
            .field("operator", &self.operator)
            .field("enabled", &self.token_digest.is_some())
            .field("session_ttl", &self.session_ttl)
            .finish()
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("command authentication is not configured")]
    Disabled,
    #[error("operator session is missing or invalid")]
    Unauthorized,
    #[error("command CSRF token is missing or invalid")]
    InvalidCsrf,
    #[error("command session expiry cannot be represented")]
    InvalidExpiry,
}

impl CommandAuth {
    pub fn disabled() -> Self {
        Self {
            operator: "unavailable".into(),
            token_digest: None,
            csrf_token: None,
            session_ttl: Duration::minutes(15),
        }
    }

    pub fn enabled(
        operator: impl Into<String>,
        session_token: &str,
        csrf_secret: &[u8],
        session_ttl: Duration,
    ) -> Result<Self, AuthError> {
        if session_token.len() < 32 || csrf_secret.len() < 32 {
            return Err(AuthError::Unauthorized);
        }
        let token_digest: [u8; 32] = Sha256::digest(session_token.as_bytes()).into();
        let mut mac =
            HmacSha256::new_from_slice(csrf_secret).map_err(|_| AuthError::Unauthorized)?;
        mac.update(b"helios-command-csrf-v1\0");
        mac.update(session_token.as_bytes());
        let csrf_token = hex::encode(mac.finalize().into_bytes());
        Ok(Self {
            operator: operator.into(),
            token_digest: Some(token_digest),
            csrf_token: Some(csrf_token),
            session_ttl,
        })
    }

    pub fn authenticate(&self, headers: &HeaderMap) -> Result<&str, AuthError> {
        let expected = self.token_digest.as_ref().ok_or(AuthError::Disabled)?;
        let candidate = cookie(headers, SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
        let digest: [u8; 32] = Sha256::digest(candidate.as_bytes()).into();
        if digest.ct_eq(expected).unwrap_u8() != 1 {
            return Err(AuthError::Unauthorized);
        }
        Ok(&self.operator)
    }

    pub fn is_enabled(&self) -> bool {
        self.token_digest.is_some()
    }

    pub fn verify_csrf(&self, headers: &HeaderMap) -> Result<(), AuthError> {
        let expected = self.csrf_token.as_ref().ok_or(AuthError::Disabled)?;
        let provided = headers
            .get("x-helios-csrf")
            .and_then(|value| value.to_str().ok())
            .ok_or(AuthError::InvalidCsrf)?;
        let expected_digest: [u8; 32] = Sha256::digest(expected.as_bytes()).into();
        let provided_digest: [u8; 32] = Sha256::digest(provided.as_bytes()).into();
        if provided_digest.ct_eq(&expected_digest).unwrap_u8() != 1 {
            return Err(AuthError::InvalidCsrf);
        }
        Ok(())
    }

    pub fn session(&self, headers: &HeaderMap) -> Result<CommandSession, AuthError> {
        let operator = self.authenticate(headers)?.to_owned();
        let expires_at = (OffsetDateTime::now_utc() + self.session_ttl)
            .format(&Rfc3339)
            .map_err(|_| AuthError::InvalidExpiry)?;
        Ok(CommandSession {
            schema_version: 1,
            operator,
            expires_at,
            csrf_token: self.csrf_token.clone().ok_or(AuthError::Disabled)?,
        })
    }
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then(|| value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";
    const SECRET: &[u8] = b"abcdef0123456789abcdef0123456789";

    fn auth() -> CommandAuth {
        CommandAuth::enabled("operator@example.com", TOKEN, SECRET, Duration::minutes(5)).unwrap()
    }

    fn headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("other=1; {SESSION_COOKIE}={token}; theme=dark"))
                .unwrap(),
        );
        headers
    }

    #[test]
    fn cookie_authentication_and_csrf_are_independent() {
        let auth = auth();
        let mut request = headers(TOKEN);
        assert_eq!(auth.authenticate(&request).unwrap(), "operator@example.com");
        assert_eq!(auth.verify_csrf(&request), Err(AuthError::InvalidCsrf));
        request.insert(
            "x-helios-csrf",
            HeaderValue::from_str(auth.csrf_token.as_ref().unwrap()).unwrap(),
        );
        auth.verify_csrf(&request).unwrap();
    }

    #[test]
    fn debug_never_contains_token_or_csrf_secret() {
        let rendered = format!("{:?}", auth());
        assert!(!rendered.contains(TOKEN));
        assert!(!rendered.contains("abcdef0123456789"));
    }

    #[test]
    fn disabled_and_wrong_token_fail_closed() {
        assert_eq!(
            CommandAuth::disabled().authenticate(&headers(TOKEN)),
            Err(AuthError::Disabled)
        );
        assert_eq!(
            auth().authenticate(&headers("wrong")),
            Err(AuthError::Unauthorized)
        );
    }
}
