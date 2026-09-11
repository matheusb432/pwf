# pwf-app

[![crates.io version](https://img.shields.io/crates/v/pwf-app.svg)](https://crates.io/crates/pwf-app)

CLI app to track pending work and study notes from your projects with as little effort as possible.

This runs locally and is offline only, and at the moment I don't plan on adding an adapter/plugin to support integration with a cloud service.
However, considering the data of the app is almost entirely markdown notes, it should be simple to use it externally.

## Getting started

Register a project with an obsidian vault:

```bash
# creates in vault ~/my-vault, and pwf will sabe tasks and notes in ~/my-vault/tasks
pwf project add-vault ~/my-vault --id app --tasks-path tasks --source-path ~/code/my-app
```

## References

- [Installation and updates](https://github.com/matheusb432/pwf/blob/main/wiki/installation-and-update.md)
- [Projects](https://github.com/matheusb432/pwf/blob/main/wiki/projects.md)
- [Tasks](https://github.com/matheusb432/pwf/blob/main/wiki/tasks.md)
- [Notes](https://github.com/matheusb432/pwf/blob/main/wiki/notes.md)
- [Sessions](https://github.com/matheusb432/pwf/blob/main/wiki/session.md)
- [Obsidian integration](https://github.com/matheusb432/pwf/blob/main/wiki/obsidian.md)
- [Troubleshooting](https://github.com/matheusb432/pwf/blob/main/wiki/troubleshooting.md)
- [Design](https://github.com/matheusb432/pwf/blob/main/wiki/design.md)
