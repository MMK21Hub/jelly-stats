use reqwest::blocking::Client as HttpClient;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

pub mod error {
    use crate::jelly::JellyAPIError;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum JellyError {
        #[error("http error: {0}")]
        Http(#[from] reqwest::Error),

        #[error("api error: {0}")]
        Api(JellyAPIError),

        #[error("invalid header")]
        InvalidHeader,
    }
}

use error::JellyError;
use thiserror::Error;

#[derive(Debug, Error)]
enum JellyAPIError {
    #[error("endpoint not found")]
    EndpointNotFound,
    #[error("invalid API token")]
    InvalidAPIToken,
    #[error("{0}")]
    UnknownError(String),
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
}

impl From<ErrorResponse> for JellyAPIError {
    fn from(value: ErrorResponse) -> Self {
        match value.error.as_str() {
            "Endpoint not found" => Self::EndpointNotFound,
            "Invalid API token" => Self::InvalidAPIToken,
            other => Self::UnknownError(other.to_owned()),
        }
    }
}

#[derive(Clone)]
pub struct JellyClient {
    base_url: String,
    http: HttpClient,
}

impl JellyClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, JellyError> {
        let mut headers = HeaderMap::new();

        let mut auth_value = HeaderValue::from_str(&format!("Bearer {}", api_key.into()))
            .map_err(|_| JellyError::InvalidHeader)?;
        auth_value.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth_value);

        let http = HttpClient::builder().default_headers(headers).build()?;

        Ok(Self {
            base_url: base_url.into(),
            http,
        })
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, JellyError> {
        let url = self.url(path);
        let response = self.http.get(&url).send()?;
        let result: Result<T, JellyError> = if response.status().is_success() {
            Ok(response.json::<T>()?)
        } else {
            let error = response.json::<ErrorResponse>()?;
            Err(JellyError::Api(error.into()))
        };
        result
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}
