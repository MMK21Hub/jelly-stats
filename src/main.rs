use anyhow::{Context, Ok, Result};
use jelly_stats::jelly::{Conversation, ConversationListOptions, ConversationStatus, JellyClient};
use url::Url;

fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let client = JellyClient::new(
        Url::parse(
            &std::env::var("JELLY_API_URL").unwrap_or("https://app.letsjelly.com/api".into()),
        )?,
        std::env::var("JELLY_API_KEY").context("JELLY_API_KEY must be set")?,
    )?;

    let convos: Vec<Conversation> = client
        .all_conversations(&ConversationListOptions {
            // label_id: Some("".to_string()),
            mailbox_id: Some("stardance".to_string()),
            status: Some(ConversationStatus::Open),
            ..Default::default()
        })?
        .into_iter()
        .filter(|c| c.labels.len() == 0)
        .collect();

    println!("Conversations: {:#?}", convos);
    println!("Conversations: {:#?}", convos.len());

    Ok(())
}
