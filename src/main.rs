use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    sync::{Arc, RwLock},
    thread,
};

#[cfg(feature = "slack")]
mod slack;

#[cfg(feature = "slack")]
use slack::SlackConfig;

use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use chrono::{NaiveDate, Utc};
use jelly_stats::jelly::{
    Conversation, ConversationDetail, ConversationListOptions, ConversationStatus, JellyClient,
    Sender,
};
use log::{debug, info};
use serde::Serialize;
use url::Url;

#[derive(Clone, Serialize, Debug)]
struct Stats {
    open_conversations: u64,
    total_conversations: u64,
    new_conversations_last_24h: u64,
    new_conversations_per_day: BTreeMap<NaiveDate, u64>,
    hang_time: Option<HangTimeStats>,
    leaderboard: Vec<LeaderboardEntry>,
}

#[derive(Debug, Serialize, Clone)]
struct HangTimeStats {
    mean_seconds: f64,
    median_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeaderboardEntry {
    pub name: String,
    pub count: u64,
}

type SharedStats = Arc<RwLock<Option<Stats>>>;

fn hang_time_seconds(detail: &ConversationDetail) -> Option<i64> {
    let first_message = detail
        .messages
        .iter()
        .min_by(|left, right| left.sent_at.cmp(&right.sent_at))?;
    let first_response = detail
        .messages
        .iter()
        .filter(|message| message.sent_at > first_message.sent_at)
        .find(|message| matches!(message.sender, Some(Sender::Member { .. })))?;

    Some(
        first_response
            .sent_at
            .signed_duration_since(first_message.sent_at)
            .num_seconds(),
    )
}

fn calculate_hang_times(values: &[i64]) -> Option<HangTimeStats> {
    if values.is_empty() {
        return None;
    }

    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mean_seconds = sorted.iter().sum::<i64>() as f64 / sorted.len() as f64;
    let median_seconds = if sorted.len() % 2 == 0 {
        let middle = sorted.len() / 2;
        (sorted[middle - 1] as f64 + sorted[middle] as f64) / 2.0
    } else {
        sorted[sorted.len() / 2] as f64
    };

    Some(HangTimeStats {
        mean_seconds,
        median_seconds,
    })
}

async fn metrics(State(stats): State<SharedStats>) -> String {
    let s = stats.read().unwrap();
    match s.as_ref() {
        Some(s) => {
            let hang_times = match &s.hang_time {
                Some(hang_time) => format!(
                    "\
                    # HELP jelly_hang_time_seconds_mean Mean hang time between the first email and the first staff reply\n\
                    # TYPE jelly_hang_time_seconds_mean gauge\n\
                    jelly_hang_time_seconds_mean {}\n\
                    # HELP jelly_hang_time_seconds_median Median hang time between the first email and the first staff reply\n\
                    # TYPE jelly_hang_time_seconds_median gauge\n\
                    jelly_hang_time_seconds_median {}\n\
                    ",
                    hang_time.mean_seconds, hang_time.median_seconds
                ),
                None => format!(""),
            };

            format!(
                "\
                # HELP jelly_open_conversations Current number of open conversations\n\
                # TYPE jelly_open_conversations gauge\n\
                jelly_open_conversations {}\n\
                # HELP jelly_total_conversations Current number of conversations\n\
                # TYPE jelly_total_conversations gauge\n\
                jelly_total_conversations {}\n\
                {}\n\
                ",
                s.open_conversations, s.total_conversations, hang_times
            )
        }
        None => format!(""),
    }
}

async fn stats_json(State(stats): State<SharedStats>) -> impl IntoResponse {
    let stats = stats.read().unwrap();

    match &*stats {
        Some(stats) => (StatusCode::OK, Json(stats)).into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "initial scrape has not completed"
            })),
        )
            .into_response(),
    }
}

fn scrape_loop(stats: SharedStats) -> Result<()> {
    let client = JellyClient::new(
        Url::parse(
            &std::env::var("JELLY_API_URL").unwrap_or("https://app.letsjelly.com/api".into()),
        )?,
        std::env::var("JELLY_API_KEY").context("JELLY_API_KEY must be set")?,
    )?;
    let target_mailbox = std::env::var("JELLY_MAILBOX").ok();
    let scrape_interval = std::env::var("SCRAPE_INTERVAL")
        .ok()
        .map(|s| humantime::parse_duration(&s))
        .transpose()
        .context("SCRAPE_INTERVAL must be a valid duration (e.g. 10m, 60s)")?
        .unwrap_or(std::time::Duration::from_mins(10));
    let max_conversations = std::env::var("MAX_CONVERSATIONS")
        .ok()
        .map(|s| s.parse::<u32>())
        .transpose()
        .context("MAX_CONVERSATIONS must be a valid positive integer")?;
    if let Some(slug) = &target_mailbox {
        info!("Using Jelly mailbox: {}", slug);
    } else {
        info!("No Jelly mailbox specified, fetching all conversations");
    }
    if let Some(max) = max_conversations {
        info!("Max conversations limit set to {}", max);
    }

    loop {
        info!(
            "Fetching jelly statistics at {}",
            Utc::now().format("%Y-%m-%d %H:%M:%S")
        );
        let conversations: Vec<Conversation> = client
            .all_conversations(
                &ConversationListOptions {
                    mailbox_id: target_mailbox.clone(),
                    limit: Some(100),
                    ..Default::default()
                },
                max_conversations,
            )?
            .into_iter()
            .collect();

        let now = Utc::now();
        let mut new_conversations_per_day = BTreeMap::new();
        let mut new_conversations_last_24h = 0;
        let mut hang_times = Vec::new();
        let mut member_message_counts: HashMap<String, (String, u64)> = HashMap::new();
        for convo in conversations.iter() {
            // Bucket conversations into the date they were created
            let day = convo.created_at.date_naive();
            *new_conversations_per_day.entry(day).or_insert(0) += 1;
            // Also track the new convos in the past 24h
            if now - convo.created_at < chrono::Duration::hours(24) {
                new_conversations_last_24h += 1;
            }

            let detail = client.get_conversation(&convo.id)?;
            if let Some(hang_time) = hang_time_seconds(&detail) {
                hang_times.push(hang_time);
            }
            for msg in &detail.messages {
                if let Some(Sender::Member { ref id, ref name, .. }) = msg.sender
                    && now - msg.sent_at < chrono::Duration::hours(24)
                {
                    let entry = member_message_counts
                        .entry(id.clone())
                        .or_insert((name.clone(), 0));
                    entry.1 += 1;
                }
            }
        }

        let hang_time = calculate_hang_times(&hang_times);

        let leaderboard = {
            let mut entries: Vec<LeaderboardEntry> = member_message_counts
                .into_values()
                .map(|(name, count)| LeaderboardEntry { name, count })
                .collect();
            entries.sort_by(|a, b| b.count.cmp(&a.count));
            entries.truncate(10);
            entries
        };

        {
            let new_stats = Stats {
                open_conversations: conversations
                    .iter()
                    .filter(|c| c.status == ConversationStatus::Open)
                    .count() as u64,
                total_conversations: conversations.len() as u64,
                new_conversations_last_24h,
                new_conversations_per_day,
                hang_time,
                leaderboard,
            };
            *stats.write().unwrap() = Some(new_stats.clone());
            debug!("Latest stats: {:?}", new_stats);
        }

        info!(
            "Successfully fetched statistics, {} conversations found",
            conversations.len()
        );

        thread::sleep(scrape_interval);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    pretty_env_logger::init();
    let stats: SharedStats = Arc::new(RwLock::new(None));

    let stats_clone = stats.clone();
    std::thread::spawn(move || {
        let result = scrape_loop(stats_clone);
        match result {
            Ok(_) => {}
            Err(error) => {
                log::error!("Error in scrape loop: {}", error);
                log::error!("{:#?}", error);
                std::process::exit(1);
            }
        }
    });

    #[cfg(feature = "slack")]
    {
        let stats_for_slack = stats.clone();
        tokio::spawn(async move {
            use std::time::Duration;
            use chrono::Utc;

            let slack_config = SlackConfig::from_env()
                .expect("Failed to load Slack config");
            let mut last_posted_date: Option<chrono::NaiveDate> = None;

            loop {
                let now = Utc::now();
                let today = now.date_naive();
                let current_time = now.time();

                if current_time >= slack_config.post_time
                    && last_posted_date != Some(today)
                {
                    let leaderboard = stats_for_slack
                        .read()
                        .unwrap()
                        .as_ref()
                        .map(|s| s.leaderboard.clone())
                        .unwrap_or_default();

                    if !leaderboard.is_empty() {
                        if let Err(e) = slack::post_leaderboard(&slack_config, &leaderboard).await {
                            log::error!("Failed to post leaderboard to Slack: {}", e);
                        }
                        last_posted_date = Some(today);
                    }
                }

                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });
    }

    let app = Router::new()
        .route("/metrics", get(metrics))
        .route("/stats", get(stats_json))
        .with_state(stats);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    axum::serve(
        tokio::net::TcpListener::bind(addr)
            .await
            .context("failed to bind to port")?,
        app,
    )
    .await
    .context("failed to start server")?;

    Ok(())
}
