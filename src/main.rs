use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use chrono::Utc;
use jelly_stats::jelly::{ConversationListOptions, JellyClient};
use log::{debug, info};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT_LANGUAGE, HeaderMap, HeaderValue};
use serde::Serialize;
use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
    thread,
};
use url::Url;

#[cfg(feature = "slack")]
mod slack;
#[cfg(feature = "slack")]
use slack::SlackConfig;

#[derive(Clone, Serialize, Debug)]
struct Stats {
    open_conversations: u64,
    total_conversations: u64,
    awaiting_reply: u64,
    // TODO consider adding these back
    // new_conversations_last_24h: u64,
    // new_conversations_per_day: BTreeMap<NaiveDate, u64>,
    // hang_time: Option<HangTimeStats>,
    // leaderboard: Vec<LeaderboardEntry>,
}

// #[derive(Debug, Serialize, Clone)]
// struct HangTimeStats {
//     mean_seconds: f64,
//     median_seconds: f64,
// }

#[derive(Debug, Clone, Serialize)]
pub struct LeaderboardEntry {
    pub name: String,
    pub count: u64,
}

type SharedStats = Arc<RwLock<Option<Stats>>>;

async fn metrics(State(stats): State<SharedStats>) -> String {
    let s = stats.read().unwrap();
    match s.as_ref() {
        Some(s) => {
            format!(
                "\
                # HELP jelly_open_conversations Current number of open conversations\n\
                # TYPE jelly_open_conversations gauge\n\
                jelly_open_conversations {}\n\
                # HELP jelly_total_conversations Current number of conversations\n\
                # TYPE jelly_total_conversations gauge\n\
                jelly_total_conversations {}\n\
                # HELP jelly_awaiting_reply Current number of conversations awaiting reply from the team\n\
                # TYPE jelly_awaiting_reply gauge\n\
                jelly_awaiting_reply {}\n
                ",
                s.open_conversations, s.total_conversations, s.awaiting_reply
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
    let jelly = JellyClient::new(
        Url::parse(
            &std::env::var("JELLY_API_URL").unwrap_or("https://app.letsjelly.com/api".into()),
        )?,
        std::env::var("JELLY_API_KEY").context("JELLY_API_KEY must be set")?,
    )?;
    let jelly_base_url =
        Url::parse(&std::env::var("JELLY_APP_URL").unwrap_or("https://app.letsjelly.com/".into()))?;
    let jelly_team_url = jelly_base_url
        .join(&(std::env::var("JELLY_TEAM").context("JELLY_TEAM must be set")? + "/"))?;
    let session_token =
        std::env::var("JELLY_SESSION_TOKEN").context("JELLY_SESSION_TOKEN must be set")?;
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
    let http = Client::builder()
        .user_agent("jelly-stats")
        .default_headers(default_headers)
        .cookie_provider(Arc::new(cookies))
        .build()?;

    loop {
        info!(
            "Fetching jelly statistics at {}",
            Utc::now().format("%Y-%m-%d %H:%M:%S")
        );

        let statistics_url = jelly_team_url.join("statistics")?;
        debug!("Fetching Jelly statistics page at {}", statistics_url);
        let response = http.get(statistics_url).send()?.text()?;

        let dom = tl::parse(&response, tl::ParserOptions::default())
            .context("failed to parse HTML response")?;
        let parser = dom.parser();
        let statistics_cards = dom
            .query_selector(".statistics-card")
            .context("failed to find statistics cards in HTML")?;
        debug!(
            "Found {} statistics cards",
            statistics_cards.clone().count()
        );

        // let mut new_conversations: Option<u64> = None;
        let mut open_now: Option<u64> = None;
        let mut awaiting_reply: Option<u64> = None;

        for card in statistics_cards {
            let card = card
                .get(parser)
                .and_then(|e| e.as_tag())
                .context("failed to get statistics card element")?;
            debug!(
                "Found statistics card: {}",
                card.outer_html(parser)
                    .split_whitespace()
                    .collect::<Vec<&str>>()
                    .join(" ")
            );
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
            let value: Result<u64, _> = value.inner_text(parser).parse();
            if label.contains("new conversations") {
                // new_conversations =
                //     Some(value.context("failed to parse new conversations stat value")?);
            } else if label.contains("open now") {
                open_now = Some(value.context("failed to parse open now stat value")?);
            } else if label.contains("awaiting reply") {
                awaiting_reply = Some(value.context("failed to parse awaiting reply stat value")?);
            }
        }

        debug!("Counting total conversations using Jelly API");
        let total_conversations = jelly
            .count_conversations(&ConversationListOptions::default())
            .context("failed to count total conversations using Jelly API")?;

        let new_stats = Stats {
            open_conversations: open_now.context("failed to fetch open conversations count")?,
            total_conversations: total_conversations as u64,
            awaiting_reply: awaiting_reply.context("failed to fetch awaiting reply count")?,
        };

        info!("Successfully fetched statistics, {total_conversations} conversations found");
        debug!("Statistics: {:?}", new_stats);

        stats.write().unwrap().replace(new_stats);
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
