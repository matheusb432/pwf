# obsidian

register a project with `pwf project add-vault`:

```bash
pwf project add-vault ~/notes --id app --tasks-path projects/my-app --source-path ~/code/my-app
```

> [!IMPORTANT]
> the vault root must contain a `.obsidian` folder.
`--tasks-path` selects a relative folder inside the vault. `projects/my-app` gives the project the title `my-app`.

from the vault root, the path can be omitted:

```bash
pwf project add-vault --id app --tasks-path projects/my-app
```

`--source-path` is optional. without it, tasks and notes work, but `pwf session` fails until a source directory is set:

```bash
pwf project edit app --source ~/code/my-app
```

use `pwf project edit app --clear-source` to remove the source directory.

## task removal

> [!CAUTION]
> projects registered with `add-vault` store the vault root path in `projects.obsidian_vault`.
> removing a task moves its note to that vault's `.trash` folder. the folder must already exist; if it is missing, removal fails before changing the task.
> when a filename is taken, the next available numbered name is used, such as `APP-0001 (1).md`.
