# Obsidian

Register the vault through [`pwf project add-vault`](projects.md).

> [!IMPORTANT]
> The vault root must contain a `.obsidian` folder.

PWF stores task and study-note Markdown in the configured task directory. You can edit these files with Obsidian or any text editor.

## Task removal

> [!CAUTION]
> Removing a task from a vault project moves its note to the vault's `.trash` folder. The folder must already exist. When a filename is taken, PWF uses the next numbered name, such as `APP-0001 (1).md`.

## Generated index

Enable an index when you want one page with links to every task and study note:

```bash
pwf project edit app --snapshot-enabled true
```

The server writes `pwf-index.md` on startup and refreshes it about once a minute. Manual edits to this file are replaced on a later refresh. Disabling snapshots leaves the last generated file in place.

The authored `<project title>.md` page remains available for your own content. Task and note commands leave it unchanged.
