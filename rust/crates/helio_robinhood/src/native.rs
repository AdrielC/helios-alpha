use std::io::Read;
use std::time::Duration;

use thiserror::Error;

use crate::{
    HttpMethod, HttpRequest, HttpResponse, RobinhoodTransport, TransportError,
    ROBINHOOD_CRYPTO_API_ORIGIN,
};

const MAX_RESPONSE_BYTES: u64 = 1_048_576;

#[derive(Debug, Error)]
pub enum NativeTransportBuildError {
    #[error("Robinhood HTTP timeout must be positive")]
    ZeroTimeout,
    #[error("failed to construct Robinhood HTTP client")]
    Client,
}

#[derive(Debug, Clone)]
pub struct ReqwestRobinhoodTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestRobinhoodTransport {
    pub fn try_new(timeout: Duration) -> Result<Self, NativeTransportBuildError> {
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
        Ok(Self { client })
    }
}

impl RobinhoodTransport for ReqwestRobinhoodTransport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let url = format!("{ROBINHOOD_CRYPTO_API_ORIGIN}{}", request.path_and_query);
        let mut builder = match request.method {
            HttpMethod::Get => self.client.get(url),
            HttpMethod::Post => self.client.post(url),
        };
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        if !request.body.is_empty() {
            builder = builder.body(request.body);
        }
        let response = builder.send().map_err(|_| match request.method {
            HttpMethod::Get => TransportError::Unavailable,
            HttpMethod::Post => TransportError::OutcomeUnknown,
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
