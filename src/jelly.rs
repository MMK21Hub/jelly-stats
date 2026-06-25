use chrono::{DateTime, Utc};
use error::JellyError;
use log::debug;
use reqwest::Url;
use reqwest::blocking::Client as HttpClient;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod error {
    use crate::jelly::JellyAPIError;
    use reqwest::Url;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum JellyError {
        #[error("http error: {0}")]
        Http(#[from] reqwest::Error),

        #[error("api error ({status}) on {endpoint}: {source}")]
        Api {
            status: reqwest::StatusCode,
            endpoint: Url,
            body: String,
            #[source]
            source: JellyAPIError,
        },

        #[error("invalid header")]
        InvalidHeader,

        #[error("invalid url: {0}")]
        InvalidUrl(#[from] url::ParseError),

        #[error("url encoding error: {0}")]
        UrlEncoding(#[from] serde_urlencoded::ser::Error),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationStatus {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "archived")]
    Archived,
    #[serde(rename = "snoozed")]
    Snoozed,
    #[serde(rename = "spam")]
    Spam,
    #[serde(rename = "trash")]
    Trash,
}

#[derive(Debug, Default, Clone, Serialize)]
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
    pub status: ConversationStatus,
    pub labels: Vec<ConversationLabel>,
    pub messages_count: u32,
    pub comments_count: u32,
    pub attachments_count: u32,
    pub snoozed_until: Option<DateTime<Utc>>,
    pub url: Url,
    pub markdown_url: Url,
    pub messages_url: Url,
    pub comments_url: Url,
    pub draft_reply_url: Url,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_message_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationsPage {
    pub conversations: Vec<Conversation>,
    pub next_cursor: Option<String>,
}

#[derive(Clone)]
pub struct JellyClient {
    base_url: Url,
    http: HttpClient,
}

impl JellyClient {
    pub fn new(base_url: impl Into<Url>, api_key: impl Into<String>) -> Result<Self, JellyError> {
        let mut headers = HeaderMap::new();

        let mut auth_value = HeaderValue::from_str(&format!("Bearer {}", api_key.into()))
            .map_err(|_| JellyError::InvalidHeader)?;
        auth_value.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth_value);

        let http = HttpClient::builder().default_headers(headers).build()?;

        Ok(Self {
            base_url: base_url.into().join("/api/").expect("failed to build URL"),
            http,
        })
    }

    fn get_with_query<T: DeserializeOwned>(
        &self,
        path: &str,
        query: String,
    ) -> Result<T, JellyError> {
        let path = path.trim_start_matches("/");
        let url = self.base_url.join(&format!("{}?{}", path, query))?;
        debug!("GET {}", url);
        let response = self.http.get(url.clone()).send()?;
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
        self.get_with_query("/conversations", serde_urlencoded::to_string(options)?)
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
            debug!(
                "Discovered {} conversations on page",
                page.conversations.len()
            );
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
}
