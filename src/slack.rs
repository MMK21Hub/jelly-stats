//! Slack notification module for daily leaderboard.
//!
//! Only compiled when the `slack` feature is enabled.
//! Posts a daily leaderboard of who sent the most emails in the past 24h.

use anyhow::{Context, Result};
use chrono::Utc;
use slack_api::chat::{self, PostMessageRequest};

use crate::LeaderboardEntry;

/// Slack configuration loaded from environment variables at startup.
#[derive(Clone, Debug)]
pub struct SlackConfig {
    pub token: String,
    pub channel: String,
    pub post_time: String,
}

impl SlackConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            token: std::env::var("SLACK_BOT_TOKEN")
                .context("SLACK_BOT_TOKEN must be set when using the slack feature")?,
            channel: std::env::var("SLACK_CHANNEL_ID")
                .context("SLACK_CHANNEL_ID must be set when using the slack feature")?,
            post_time: std::env::var("SLACK_POST_TIME")
                .unwrap_or_else(|_| "14:00".to_string()),
        })
    }
}

/// Posts the formatted leaderboard to the configured Slack channel.
pub async fn post_leaderboard(
    config: &SlackConfig,
    leaderboard: &[LeaderboardEntry],
) -> Result<()> {
    if leaderboard.is_empty() {
        log::info!("No leaderboard data to post — skipping Slack message");
        return Ok(());
    }

    let text = format_leaderboard(leaderboard);

    let client =
        slack_api::requests::default_client().context("Failed to create Slack HTTP client")?;

    let request = PostMessageRequest {
        channel: &config.channel,
        text: &text,
        link_names: Some(true),
        ..Default::default()
    };

    chat::post_message(&client, &config.token, &request)
        .await
        .context("Failed to post message to Slack")?;

    log::info!(
        "Posted daily leaderboard to Slack channel {}",
        config.channel
    );
    Ok(())
}

/// Formats leaderboard entries into a Slack mrkdwn message.
fn format_leaderboard(entries: &[LeaderboardEntry]) -> String {
    let date = Utc::now().format("%Y-%m-%d");
    let mut msg = format!("📊 *Daily Email Leaderboard ({})*\n", date);
    for (i, entry) in entries.iter().enumerate() {
        let medal = match i {
            0 => ":first_place_medal: ",
            1 => ":second_place_medal: ",
            2 => ":third_place_medal: ",
            _ => "",
        };
        msg.push_str(&format!(
            "{}. {}{} — {} emails\n",
            i + 1,
            medal,
            entry.name,
            entry.count
        ));
    }
    msg
}
