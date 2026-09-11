# Sessions

PWF can start an agent with one or more task notes in the initial prompt. Preview the exact launch first:

```bash
pwf session app1 --dry-run
pwf session app1
```

> [!IMPORTANT]
> Sessions require a source directory. Set one with `pwf project edit app --source ~/code/my-app`.

Sessions run in the current terminal. The default agent is Codex, and `--agent claude` selects Claude. The selected agent must already be installed.

One session can include up to five tasks from the same project. Supply them as one comma-separated value:

```bash
pwf session app1,app2
```

`pwf` makes no network requests, this command forwards command to your selected harness' CLI.
