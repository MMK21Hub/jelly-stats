use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
    thread,
};

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
}

#[derive(Debug, Serialize, Clone)]
struct HangTimeStats {
    mean_seconds: f64,
    median_seconds: f64,
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
    if let Some(slug) = &target_mailbox {
        info!("Using Jelly mailbox: {}", slug);
    } else {
        info!("No Jelly mailbox specified, fetching all conversations");
    }

    loop {
        info!(
            "Fetching jelly statistics at {}",
            Utc::now().format("%Y-%m-%d %H:%M:%S")
        );
        let conversations: Vec<Conversation> = client
            .all_conversations(&ConversationListOptions {
                mailbox_id: target_mailbox.clone(),
                limit: Some(100),
                ..Default::default()
            })?
            .into_iter()
            .filter(|c| c.labels.len() == 0)
            .collect();

        let now = Utc::now();
        let mut new_conversations_per_day = BTreeMap::new();
        let mut new_conversations_last_24h = 0;
        let mut hang_times = Vec::new();
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
        }

        let hang_time = calculate_hang_times(&hang_times);

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
