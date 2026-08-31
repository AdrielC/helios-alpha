use std::io::Read;
use std::time::Duration;

use thiserror::Error;

use crate::{
    AlpacaEnvironment, AlpacaTransport, HttpMethod, HttpRequest, HttpResponse, TransportError,
};

const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NativeTransportBuildError {
    #[error("Alpaca HTTP timeout must be positive")]
    ZeroTimeout,
    #[error("failed to construct Alpaca HTTP client")]
    Client,
}

#[derive(Debug, Clone)]
pub struct ReqwestAlpacaTransport {
    origin: &'static str,
    client: reqwest::blocking::Client,
}

impl ReqwestAlpacaTransport {
    pub fn try_new(
        environment: AlpacaEnvironment,
        timeout: Duration,
    ) -> Result<Self, NativeTransportBuildError> {
        if timeout.is_zero() {
            return Err(NativeTransportBuildError::ZeroTimeout);
        }
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| NativeTransportBuildError::Client)?;
        Ok(Self {
            origin: environment.api_origin(),
            client,
        })
    }
}

impl AlpacaTransport for ReqwestAlpacaTransport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let url = format!("{}{}", self.origin, request.path_and_query);
        let mut builder = match request.method {
            HttpMethod::Get => self.client.get(url),
            HttpMethod::Post => self.client.post(url),
            HttpMethod::Patch => self.client.patch(url),
            HttpMethod::Delete => self.client.delete(url),
        };
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        if !request.body.is_empty() {
            builder = builder.body(request.body);
        }
        let response = builder.send().map_err(|_| {
            if request.method.is_mutating() {
                TransportError::OutcomeUnknown
            } else {
                TransportError::Unavailable
            }
        })?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES)
        {
            return Err(TransportError::ResponseTooLarge);
        }
        let status = response.status().as_u16();
        let mut body = Vec::new();
        response
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|_| TransportError::Unavailable)?;
        if body.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(TransportError::ResponseTooLarge);
        }
        Ok(HttpResponse { status, body })
    }
}
