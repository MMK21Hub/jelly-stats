# `jelly-stats`

[![Docker image CI build status](https://github.com/MMK21Hub/jelly-stats/actions/workflows/build-and-push.yaml/badge.svg)](https://github.com/MMK21Hub/jelly-stats/actions/workflows/build-and-push.yaml)

> basically we use the [Jelly](https://letsjelly.com/) API to fetch all conversations, and generate some stats based on the data we see.
>
> stats are exposed via Prometheus-compatible metrics (`/metrics`), and a JSON endpoint (`/stats`).

## Configuration

The following environment variables are accepted:

<!-- prettier-ignore -->
| Variable                       | Description                                                                                 | Default                          |
| ------------------------------ | ------------------------------------------------------------------------------------------- | -------------------------------- |
| `JELLY_API_KEY` **(required)** | A valid Jelly API token for your Jelly workspace. (Note that only admins can use API keys.) | N/A                              |
| `RUST_LOG` (recommended)       | Set the log level. Recommend setting to `info`.                                             | Empty (no logs)                  |
| `JELLY_MAILBOX` (recommended)  | The slug of the mailbox to fetch conversations from (e.g. `stardance`)                      | Empty (Fetch from all mailboxes) |
| `SCRAPE_INTERVAL`              | How long to wait between scrapes of the Jelly API. Jelly may rate-limit you if you scrape too frequently. Parsed using [`humantime`](https://docs.rs/humantime/latest/humantime/fn.parse_duration.html). | `10m` (10 minutes) |
| `JELLY_API_URL`                | The base URL of the Jelly API.                                                              | <https://app.letsjelly.com/api>  |

## Limitations

Currently, stats will only be calculated on untagged conversations.

## Public instance

Jelly stats for the stardance mailbox are available at:

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
