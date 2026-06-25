use anyhow::{Context, Ok, Result};
use jelly_stats::jelly::{ConversationListOptions, JellyClient};

fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let client = JellyClient::new(
        std::env::var("JELLY_API_URL").unwrap_or("https://app.letsjelly.com/api".to_string()),
        std::env::var("JELLY_API_KEY").context("JELLY_API_KEY must be set")?,
    )?;

    let convos = client.count_conversations(&ConversationListOptions {
        // label_id: Some("".to_string()),
        mailbox_id: Some("stardance".to_string()),
        ..Default::default()
    });

    println!("Conversations: {:#?}", convos);

    Ok(())
}
