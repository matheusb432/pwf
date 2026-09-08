# Installation and update

to install the `pwf` cli:

```bash
cargo install pwf-app --locked
pwf server install
pwf server --help # use to find other cmds
```

`pwf server install` registers startup at login and starts the server. the service is named `pwf-server`.

on Linux this is a user systemd service, on macOS a launch agent, and on Windows a scheduled task for your user.

## Updating

```bash
pwf server stop
cargo install pwf-app --locked
pwf server start
```
