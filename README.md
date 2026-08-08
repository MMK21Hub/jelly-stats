# `jelly-stats`

[![Docker image CI build status](https://github.com/MMK21Hub/jelly-stats/actions/workflows/build-and-push.yaml/badge.svg)](https://github.com/MMK21Hub/jelly-stats/actions/workflows/build-and-push.yaml)

> Basically we scrape [Jelly](https://letsjelly.com/)'s admin-facing statistics page to steal some of its statistics, and query the API to get a count of total conversations.
>
> Stats are exposed via Prometheus-compatible metrics (`/metrics`), and a JSON endpoint (`/stats`). You can use Prometheus (or similar) to keep track of these over time, and/or derive stats like "number of incoming emails per day".

## Configuration

The following environment variables are accepted:

<!-- prettier-ignore -->
| Variable                       | Description                                                                                 | Default                          |
| ------------------------------ | ------------------------------------------------------------------------------------------- | -------------------------------- |
| `JELLY_API_KEY` **(required)** | A valid Jelly API token for your Jelly workspace. Note that only admins can create or use API keys. | N/A |
| `JELLY_SESSION_TOKEN` **(required)** | A Jelly session token used for scraping. Obtained from the `current_user_session_token` browser cookie. Should look like URL-encoded base64. | N/A |
| `JELLY_TEAM` **(required)** | The slug for your Jelly team (as seen in browser URLs when logged in) e.g. `hack-club` | N/A |
| `RUST_LOG` (recommended)       | Set the log level. Recommend setting to `info`.                                             | Empty (no logs)                  |
| `SCRAPE_INTERVAL`              | How long to wait between scrapes of the Jelly API. Jelly may rate-limit you if you scrape too frequently. Parsed using [`humantime`](https://docs.rs/humantime/latest/humantime/fn.parse_duration.html). | `10m` (10 minutes) |
| `JELLY_API_URL` | The base URL of the Jelly API. | <https://app.letsjelly.com/api>  |
| `JELLY_APP_URL` | The base URL of the Jelly web app. | <https://app.letsjelly.com/>  |
| `SLACK_BOT_TOKEN`               | Slack bot token (starts with `xoxb-...`). Only used when built with the `slack` feature.    | N/A                              |
| `SLACK_CHANNEL_ID`              | Slack channel ID to post the daily leaderboard to (e.g. `C0123456789`). Only used when built with the `slack` feature. | N/A |
| `SLACK_POST_TIME`               | UTC time (HH:MM) to post the daily leaderboard. Defaults to the time of startup. Only used when built with the `slack` feature. | Startup time |
| `MAX_CONVERSATIONS`             | Stop pagination after discovering this many conversations. Useful for testing to avoid long API fetches. | Empty (no limit) |

On startup, environment variables are automatically loaded from a `.env` file in the working directory, but if your deployment platform (e.g. Coolify, Docker Compose) has a way to set environment variables you probably want to use that.

## Public instance

Jelly stats for the Stardance inbox are available at:

- <https://jelly-stats.slevel.xyz/metrics>
- <https://jelly-stats.slevel.xyz/stats>

## Self-hosting

You can use Docker! Here's an example Compose file:

```yaml
services:
  jelly-stats:
    image: ghcr.io/mmk21hub/jelly-stats:latest
    restart: unless-stopped
    environment:
      RUST_LOG: info
      JELLY_API_KEY: abcAAAxyz
      JELLY_MAILBOX: stardance
    ports:
      - "3010:3000"
```

Adjust to your needs, e.g. by changing the `3010` to your preferred port.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Author

Available under the MIT License.

Developed by Mish for [Hack Club](https://hackclub.com/) and the [Hack Club Stardance Challenge](https://stardance.space/r-c7t38).

If you're a teen and reading this, you should check out [Stardance](https://stardance.space/r-c7t38)! (running June&ndash;Sept 2026)
