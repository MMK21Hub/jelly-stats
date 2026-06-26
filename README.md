# `jelly-stats`

[![Docker image CI build status](https://github.com/MMK21Hub/jelly-stats/actions/workflows/build-and-push.yaml/badge.svg)](https://github.com/MMK21Hub/jelly-stats/actions/workflows/build-and-push.yaml)

> basically we use the [Jelly](https://letsjelly.com/) API to fetch all conversations, and generate some stats based on the data we see.

> stats are exposed via Prometheus-compatible metrics (`/metrics`), and a JSON endpoint (`/stats`).

## Configuration

The following environment variables are accepted:

| Variable                       | Description                                                                                 | Default                          |
| ------------------------------ | ------------------------------------------------------------------------------------------- | -------------------------------- |
| `JELLY_API_KEY` **(required)** | A valid Jelly API token for your Jelly workspace. (Note that only admins can use API keys.) | _None_                           |
| `JELLY_API_URL`                | The base URL of the Jelly API.                                                              | <https://app.letsjelly.com/api>  |
| `JELLY_MAILBOX`                | The slug of the mailbox to fetch conversations from (e.g. `stardance`)                      | Empty (Fetch from all mailboxes) |
| `RUST_LOG`                     | Set the log level. Recommend setting to `info`.                                             | Empty (no logs)                  |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Author

Available under the MIT License.

Developed by Mish for [Hack Club](https://hackclub.com/) and the [Hack Club Stardance Challenge](https://stardance.space/r-c7t38).

If you're a teen and reading this, you should check out [Stardance](https://stardance.space/r-c7t38)! (running June&ndash;Sept 2026)
