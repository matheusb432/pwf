# pwf

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

### Without Obsidian

An obsidian vault is optional, you can use:

```bash
pwf project add <json>
```

Though I recommend using obsidian to get the most of the files' format and the wikilinks that link tasks with the `blocked_by` feature.

## References

- [installation and updates](https://github.com/matheusb432/pwf/blob/main/wiki/installation-and-update.md)
- [usage](https://github.com/matheusb432/pwf/blob/main/wiki/usage.md)
- [obsidian integration](https://github.com/matheusb432/pwf/blob/main/wiki/obsidian.md)
- [design](https://github.com/matheusb432/pwf/blob/main/wiki/design.md)
