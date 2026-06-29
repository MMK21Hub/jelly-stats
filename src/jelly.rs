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

        #[error("error decoding response from url ({endpoint}): {source}")]
        Decode {
            endpoint: Url,
            body: String,
            #[source]
            source: serde_json::Error,
        },

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
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub subject: String,
    pub inbound: bool,
    pub sent_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub url: Url,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub cc: Option<Vec<String>>,
    pub html_body: String,
    pub text_body: String,
    pub attachments_count: u64,
    pub attachments: Vec<Attachment>,
    pub sender: Option<Sender>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Sender {
    Member {
        id: u64,
        name: String,
        email: String,
    },
    Contact {
        email: String,
    },
    System,
}

#[derive(Debug, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub byte_size: u64,
    pub inline: bool,
    pub url: Url,
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

#[derive(Deserialize)]
pub struct ConversationDetail {
    #[serde(flatten)]
    pub conversation: Conversation,
    pub messages: Vec<Message>,
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

    fn get<T, Q>(&self, path: &str, query: &Q) -> Result<T, JellyError>
    where
        T: DeserializeOwned,
        Q: Serialize,
    {
        let path = path.trim_start_matches("/");
        let url = self.base_url.join(path)?;
        debug!("GET {}", url);
        let response = self.http.get(url.clone()).query(query).send()?;
        let status = response.status();
        let body = response.text()?;
        let result: Result<T, JellyError> = if status.is_success() {
            serde_json::from_str(&body).map_err(|source| JellyError::Decode {
                endpoint: url.clone(),
                body: body.clone(),
                source,
            })
        } else {
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
        self.get("/conversations", options)
    }

    pub fn get_conversation(&self, id: &str) -> Result<ConversationDetail, JellyError> {
        self.get(&format!("/conversations/{}", id), &())
    }

    fn for_each_conversation_page<F>(
        &self,
        options: &ConversationListOptions,
        mut callback: F,
    ) -> Result<(), JellyError>
    where
        F: FnMut(ConversationsPage),
    {
        let mut page_options = options.clone();
        loop {
            let page = self.list_conversations(&page_options)?;
            let next_cursor = page.next_cursor.clone();
            callback(page);

            if let Some(next_cursor) = next_cursor.filter(|next_cursor| !next_cursor.is_empty()) {
                page_options.cursor = Some(next_cursor);
            } else {
                break;
            }
        }
        Ok(())
    }

    pub fn count_conversations(
        &self,
        options: &ConversationListOptions,
    ) -> Result<usize, JellyError> {
        let mut count = 0usize;
        self.for_each_conversation_page(options, |page| {
            count += page.conversations.len();
        })?;
        Ok(count)
    }

    pub fn all_conversations(
        &self,
        options: &ConversationListOptions,
    ) -> Result<Vec<Conversation>, JellyError> {
        let mut conversations = Vec::new();

        self.for_each_conversation_page(options, |page| {
            debug!(
                "Discovered {} conversations on page",
                page.conversations.len()
            );
            conversations.extend(page.conversations);
        })?;

        Ok(conversations)
    }
}
