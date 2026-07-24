//! Slack notification module for daily leaderboard.
//!
//! Only compiled when the `slack` feature is enabled.
//! Posts a daily leaderboard of who sent the most emails in the past 24h.

use anyhow::{Context, Result};
use chrono::{NaiveTime, Utc};
use slacko::{AuthConfig, SlackClient};

use crate::LeaderboardEntry;

/// Slack configuration loaded from environment variables at startup.
#[derive(Clone, Debug)]
pub struct SlackConfig {
    pub token: String,
    pub channel: String,
    pub post_time: NaiveTime,
}

impl SlackConfig {
    pub fn from_env() -> Result<Self> {
        let token = std::env::var("SLACK_BOT_TOKEN")
            .context("SLACK_BOT_TOKEN must be set when using the slack feature")?;
        let channel = std::env::var("SLACK_CHANNEL_ID")
            .context("SLACK_CHANNEL_ID must be set when using the slack feature")?;
        let post_time = std::env::var("SLACK_POST_TIME")
            .ok()
            .map(|s| NaiveTime::parse_from_str(&s, "%H:%M")
                .context("SLACK_POST_TIME must be in HH:MM format"))
            .transpose()?
            .unwrap_or_else(|| Utc::now().time());

        Ok(Self { token, channel, post_time })
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
        SlackClient::new(AuthConfig::bot(&config.token)).context("Failed to create Slack client")?;

    client
        .chat()
        .post_message(&config.channel, &text)
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
