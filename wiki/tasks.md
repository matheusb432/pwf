# Tasks

Tasks are stored as Markdown with goals and optional context/constraints/done when sections.

> [!NOTE]
> **On structure**
>
> The goals, context, constraints and done-when structure comes from [OpenAI's best practices](https://learn.chatgpt.com/guides/best-practices) for prompting
> agents, but I've found it intuitive enough to use in work I do manually, too.

## Creating

```bash
# shorthand, where `app` is the project ID
pwf add app 'my title / my goal1 / my goal2 /c some context /n some constraint /d some done when'

# explicit fields
pwf task add app --title "my title" --goal "my goal1" --goal "my goal2" --context "some context" --constraint "some constraint" --done-when "some done when"
```

Both commands create a task like this in the project's task directory:

```md
---
id: APP-0001
status: active
title: "my title"
project: "my-app"
..
---

## Goals

- my goal1
- my goal2

...
```

## Reading and listing

Use the `app1` form or `APP-0001` for the ID:

```bash
pwf task get app1 # gets task
# ^ is aliased by:
pwf get app1

pwf task list --project app # list tasks from project with id="app"
# ^ is aliased by:
pwf app
```

> [!TIP]
> all `pwf task {verb}` commands can be aliased as `pwf {verb}`

## Lifecycle and copies

Complete, cancel or reopen a task through its note:

```bash
pwf task done app1
pwf task cancel app2 --report "reason for stopping"
pwf task reopen app1
```

Creation and completion timestamps retain the machine's numeric UTC offset.

Clone a task:

```bash
pwf task clone app1 # creates in same project as 'app'
pwf task clone app1 --project foo # creates in project with id="foo"
```

## Blockers

Link a new task to direct blockers with `--blocked-by`:

```bash
pwf task add app --title "ship feature" --blocked-by app1
# Directed Acyclic Graph (DAG) view of the task's blockers and what is blocks:
pwf task dag app2
```
