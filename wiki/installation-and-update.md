# Installation and updates

To install the `pwf` CLI:

```bash
cargo install pwf-app --locked
pwf server install
pwf --help
```

> [!NOTE]
> `pwf server install` registers startup at login and starts the `pwf-server` service.
>
> On Linux this is a user systemd service, on macOS a launch agent, and on Windows a scheduled task for your user.

> [!TIP]
> Use `pwf {noun/verb} -h/--help` to get usage help on any command.

## Updating

```bash
pwf server stop
cargo install pwf-app --locked
pwf server start
```

## Data and config

Project registrations stay in a local SQLite database. Tasks and study notes stay in their configured Markdown directories. The CLI calls the server through a Unix socket or a Windows named pipe.

| Variable | Use |
| -- | -- |
| `PWF_DATABASE_PATH` | database file, defaults to `pwf/pwf.sqlite3` under the platform local data directory |
| `PWF_RUNTIME_DIR` | absolute runtime namespace shared by the CLI and server |
| `RUST_LOG` | server log filter, defaults to `info` |

User settings live in `pwf/config.toml` under the platform config directory. On Linux this is usually `~/.config/pwf/config.toml`. See [config.example.toml](../config/config.example.toml) for the current settings.
