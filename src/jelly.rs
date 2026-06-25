use error::JellyError;
use reqwest::Url;
use reqwest::blocking::Client as HttpClient;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

pub mod error {
    use crate::jelly::JellyAPIError;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum JellyError {
        #[error("http error: {0}")]
        Http(#[from] reqwest::Error),

        #[error("api error ({status}) on {endpoint}: {source}")]
        Api {
            status: reqwest::StatusCode,
            endpoint: String,
            body: String,
            #[source]
            source: JellyAPIError,
        },

        #[error("invalid header")]
        InvalidHeader,
    }
}

#[derive(Debug, Error)]
pub enum JellyAPIError {
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

impl JellyAPIError {
    fn from_raw_body(body: &str) -> Self {
        if let Ok(error) = serde_json::from_str::<ErrorResponse>(body) {
            match error.error.as_str() {
                "Endpoint not found" => Self::EndpointNotFound,
                "Invalid API token" => Self::InvalidAPIToken,
                other => Self::UnknownError(other.to_owned()),
            }
        } else {
            Self::UnknownError(body.to_owned())
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ConversationLabel {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationStatus {
    Open,
    Archived,
    Snoozed,
    Spam,
    Trash,
}

impl ConversationStatus {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Archived => "archived",
            Self::Snoozed => "snoozed",
            Self::Spam => "spam",
            Self::Trash => "trash",
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ConversationListOptions {
    pub status: Option<ConversationStatus>,
    pub label_id: Option<String>,
    pub mailbox_id: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub status: String,
    pub labels: Vec<ConversationLabel>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationsPage {
    pub conversations: Vec<Conversation>,
    pub next_cursor: Option<String>,
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

    fn get_with_query<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, JellyError> {
        let url = self.url_with_query(path, query);
        let response = self.http.get(&url).send()?;
        let status = response.status();
        let result: Result<T, JellyError> = if status.is_success() {
            Ok(response.json::<T>()?)
        } else {
            let body = response.text()?;
            Err(JellyError::Api {
                status,
                endpoint: url,
                source: JellyAPIError::from_raw_body(&body),
                body,
            })
        };
        result
    }

    pub fn list_conversations(
        &self,
        options: &ConversationListOptions,
    ) -> Result<ConversationsPage, JellyError> {
        self.get_with_query("/conversations", &conversation_query_params(options))
    }

    pub fn count_conversations(
        &self,
        options: &ConversationListOptions,
    ) -> Result<usize, JellyError> {
        let mut count = 0usize;
        let mut page_options = options.clone();

        loop {
            let page = self.list_conversations(&page_options)?;
            count += page.conversations.len();

            if let Some(next_cursor) = page
                .next_cursor
                .filter(|next_cursor| !next_cursor.is_empty())
            {
                page_options.cursor = Some(next_cursor);
            } else {
                break;
            }
        }

        Ok(count)
    }

    pub fn all_conversations(
        &self,
        options: &ConversationListOptions,
    ) -> Result<Vec<Conversation>, JellyError> {
        let mut conversations = Vec::new();
        let mut page_options = options.clone();

        loop {
            let page = self.list_conversations(&page_options)?;
            conversations.extend(page.conversations);

            if let Some(next_cursor) = page
                .next_cursor
                .filter(|next_cursor| !next_cursor.is_empty())
            {
                page_options.cursor = Some(next_cursor);
            } else {
                break;
            }
        }

        Ok(conversations)
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn url_with_query(&self, path: &str, query: &[(&str, String)]) -> String {
        let url = self.url(path);
        Url::parse_with_params(&url, query)
            .map(|url| url.to_string())
            .unwrap_or(url)
    }
}

fn conversation_query_params(options: &ConversationListOptions) -> Vec<(&str, String)> {
    let mut query = Vec::new();
    if let Some(status) = options.status {
        query.push(("status", status.as_api_str().to_owned()));
    }
    if let Some(label_id) = options.label_id.as_ref() {
        query.push(("label_id", label_id.clone()));
    }
    if let Some(mailbox_id) = options.mailbox_id.as_ref() {
        query.push(("mailbox_id", mailbox_id.clone()));
    }
    if let Some(limit) = options.limit {
        query.push(("limit", limit.to_string()));
    }
    if let Some(cursor) = options.cursor.as_ref() {
        query.push(("cursor", cursor.clone()));
    }
    query
}
