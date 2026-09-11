# Projects

A project connects a short ID and title to a task directory. It can also keep a source directory for [agent sessions](session.md).

## Creating

Register a project in an Obsidian vault:

```bash
pwf project add-vault ~/notes --id app --tasks-path projects/my-app --source-path ~/code/my-app
```

`--tasks-path` is relative to the vault root. The title defaults to `my-app`, based on the final directory name. Omit the vault path to use the current directory. Omit `--source-path` when the project does not need agent sessions.

Obsidian is optional. A project can point directly to any task directory:

```bash
pwf project add --kind directory '{"id":"APP","title":"my-app","source":{"value":"~/code/my-app"},"tasks":{"kind":"directory","path":"~/notes/my-app"}}'
```

## Listing and reading

List projects or inspect the full record for one project:

```bash
pwf project list
pwf project get APP
```

Project changes print one summary line. Add `--json` to a mutation command when a script needs the structured result.

See [Obsidian integration](obsidian.md) for vault removal and generated snapshots.
