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
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, COOKIE, HeaderMap, HeaderValue, USER_AGENT};
use reqwest::{blocking::Client, cookie::Jar};
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
    // let client = JellyClient::new(
    //     Url::parse(
    //         &std::env::var("JELLY_API_URL").unwrap_or("https://app.letsjelly.com/api".into()),
    //     )?,
    //     std::env::var("JELLY_API_KEY").context("JELLY_API_KEY must be set")?,
    // )?;
    let jelly_team_url = Url::parse(
        format!(
            "https://app.letsjelly.com/{}",
            std::env::var("JELLY_TEAM").context("JELLY_TEAM must be set")?
        )
        .as_str(),
    )?;
    let session_token =
        std::env::var("JELLY_SESSION_TOKEN").context("JELLY_SESSION_TOKEN must be set")?;
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

    let default_headers =
        HeaderMap::from_iter(vec![(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US"))]);

    let cookies = reqwest::cookie::Jar::default();
    cookies.add_cookie_str(
        format!("current_user_session_token={}", session_token).as_str(),
        &jelly_team_url,
    );
    let client = Client::builder()
        .user_agent("jelly-stats")
        .default_headers(default_headers)
        .cookie_provider(Arc::new(cookies))
        .build()?;

    loop {
        info!(
            "Fetching jelly statistics at {}",
            Utc::now().format("%Y-%m-%d %H:%M:%S")
        );

        let response = client
            .get("https://app.letsjelly.com/hack-club-stardance/statistics")
            .send()?
            .text()?;

        println!("{}", &response);

        let dom = tl::parse(&response, tl::ParserOptions::default())
            .context("failed to parse HTML response")?;
        let parser = dom.parser();
        let statistics_cards = dom
            .query_selector(".statistics-section .statistics-card")
            .context("failed to find statistics cards in HTML")?;
        debug!(
            "Found {} statistics cards",
            statistics_cards.clone().count()
        );

        let mut new_conversations: Option<u64> = None;
        let mut open_now: Option<u64> = None;
        let mut awaiting_reply: Option<u64> = None;

        for card in statistics_cards {
            let card = card
                .get(parser)
                .and_then(|e| e.as_tag())
                .context("failed to get statistics card element")?;
            let value = card
                .query_selector(parser, ".statistics-card-value")
                .and_then(|mut iter| iter.next())
                .and_then(|node| node.get(parser))
                .context("failed to get statistics card value")?;
            let label = card
                .query_selector(parser, ".statistics-card-label")
                .and_then(|mut iter| iter.next())
                .and_then(|node| node.get(parser))
                .context("failed to get statistics card label")?;
            let label = label.inner_text(parser).to_lowercase();
            let value: u64 = value
                .inner_text(parser)
                .parse()
                .context("failed to parse statistics card value")?;
            if label.contains("new conversations") {
                new_conversations = Some(value);
            } else if label.contains("open now") {
                open_now = Some(value);
            } else if label.contains("awaiting reply") {
                awaiting_reply = Some(value);
            }
        }

        if let Some(open_convos) = open_now {
            info!("Fetched statistics, {} open conversations", open_convos);
        } else {
            log::error!("Failed to fetch open conversations count");
        }

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
            use chrono::Utc;
            use std::time::Duration;

            let slack_config = SlackConfig::from_env().expect("Failed to load Slack config");
            let mut last_posted_date: Option<chrono::NaiveDate> = None;

            loop {
                let now = Utc::now();
                let today = now.date_naive();
                let current_time = now.time();

                if current_time >= slack_config.post_time && last_posted_date != Some(today) {
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
