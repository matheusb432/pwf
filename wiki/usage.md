# Usage

register a project in an obsidian vault; change these paths for your own folders:

```bash
pwf project add-vault ~/notes --id app --tasks-path projects/my-app --source-path ~/code/my-app
```

`--tasks-path` is relative to the vault root. the title defaults to `my-app`; use `--title` to override it.
omit the vault path to use the current directory. omit `--source-path` for a project without agent sessions.

pending work, which are basically tasks, are .md files structured with goals, context, constraints and a done when section.
> [!NOTE]
> **On structure**
>
> the 'goals/context/constraint/done' idea comes from [OpenAI's best practices](https://learn.chatgpt.com/guides/best-practices) when prompting
> agents, but I've found it's a simple and general enough pattern that I've come to use to any task I do, either manually or with an AI assistant.

the cli api for reading/writing to tasks has two main formats:

```bash
# shorthand syntax, `app` is the previously added project id
pwf add app 'my title / my goal1 / my goal2 /c some context /n some constraint /d some done when'

# full syntax
pwf task add app --title "my title" --goal "my goal1" --goal "my goal2" --context "some context" --constraint "some constraint" --done-when "some done when"
```

the shorthand is what I use daily, it's convenient and much faster and nicer to write, naturally. the full syntax is best for AI agents to use.
the commands above will write this file in the project's task directory:

```md
---
id: APP-0001
status: active
title: my title
project: my-app
created_at: ..
---

## Goals

- my goal1
- my goal2

## Context

- some context

## Constraints

- some constraint

## Done When

- some done when
```

to get that task's markdown in your terminal:

```bash
pwf task get app1
```

`app1` is the short form of `APP-0001`, both work.

you can link tasks with `--blocked-by` and see their dependencies with `pwf task dag app1`.

## Sessions

> [!IMPORTANT]
> sessions require a source directory. set one with `pwf project edit app --source ~/code/my-app`.

pwf supports a very simple way to start an agent session with a task's content appended to it, serving as it's initial prompt:

```bash
pwf session app1 --dry-run # recommend running it prior to the actual dispatch
pwf session app1
```

> [!NOTE]
> sessions need the selected agent installed. They run in the current terminal.
> Use `--agent claude` to select Claude. task and note commands work offline.
>
> to reinforce, `pwf` itself **does not** make any network requests, and even this command is still offline.
> so this should work with a local model wired up in the codex harness, as expected.

see [obsidian](obsidian.md) for vault registration and task removal.

## Data and config

project registrations and mutation receipts stay in a local SQLite database.
task and study notes stay in the configured notes folders, using Obsidian Markdown.
the CLI calls the server api through a unix socket or a Windows named pipe.

| Variable | Use |
| -- | -- |
| `PWF_DATABASE_PATH` | database file, defaults to `pwf/pwf.sqlite3` under the platform local data directory |
| `PWF_RUNTIME_DIR` | absolute runtime namespace, shared by the CLI and server |
| `RUST_LOG` | server log filter, defaults to `info` |

see `pwf/config.toml` under your platform's config directory to customize pwf.
on Linux, this is usually `~/.config/pwf/config.toml`.
see [config.example.toml](../config/config.example.toml) for the available settings.
