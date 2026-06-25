use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use chrono::{NaiveDate, Utc};
use jelly_stats::jelly::{Conversation, ConversationListOptions, ConversationStatus, JellyClient};
use log::info;
use serde::Serialize;
use url::Url;

#[derive(Clone, Default, Serialize)]
struct Stats {
    open_conversations: u64,
    total_conversations: u64,
    new_conversations_last_24h: u64,
    new_conversations_per_day: BTreeMap<NaiveDate, u64>,
}

type SharedStats = Arc<RwLock<Option<Stats>>>;

async fn metrics(State(stats): State<SharedStats>) -> String {
    let s = stats.read().unwrap();
    match s.as_ref() {
        Some(s) => {
            format!(
                "\
                # TYPE jelly_open_conversations gauge\n\
                jelly_open_conversations {}\n\
                # TYPE jelly_total_conversations gauge\n\
                jelly_total_conversations {}\n\
                # TYPE up gauge\n\
                up 1\n\
                ",
                s.open_conversations, s.total_conversations
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

    loop {
        info!("Extracting jelly statistics");
        let conversations: Vec<Conversation> = client
            .all_conversations(&ConversationListOptions {
                // label_id: Some("".to_string()),
                mailbox_id: Some("stardance".to_string()),
                limit: Some(100),
                ..Default::default()
            })?
            .into_iter()
            .filter(|c| c.labels.len() == 0)
            .collect();

        let now = Utc::now();
        let mut new_conversations_per_day = BTreeMap::new();
        let mut new_conversations_last_24h = 0;
        for convo in conversations.iter() {
            // Bucket conversations into the date they were created
            let day = convo.created_at.date_naive();
            *new_conversations_per_day.entry(day).or_insert(0) += 1;
            // Also track the new convos in the past 24h
            if now - convo.created_at < chrono::Duration::hours(24) {
                new_conversations_last_24h += 1;
            }
        }

        {
            let new_stats = Stats {
                open_conversations: conversations
                    .iter()
                    .filter(|c| c.status == ConversationStatus::Open)
                    .count() as u64,
                total_conversations: conversations.len() as u64,
                new_conversations_last_24h,
                new_conversations_per_day,
            };
            *stats.write().unwrap() = Some(new_stats);
        }

        thread::sleep(Duration::from_secs(5 * 60));
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
