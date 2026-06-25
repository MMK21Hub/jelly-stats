use anyhow::{Context, Ok, Result};
use jelly_stats::jelly::JellyClient;

fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let client = JellyClient::new(
        std::env::var("JELLY_API_URL").unwrap_or("https://app.letsjellyaaa.com/api".to_string()),
        std::env::var("JELLY_API_KEY").context("JELLY_API_KEY must be set")?,
    )?;

    Ok(())
}
