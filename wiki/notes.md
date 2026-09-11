# Notes

Study notes are Markdown files stored beside a project's tasks. Their IDs use the `APP-NOTE-0001` form.

Add a note with the short title and content form:

```bash
pwf note add app 'sqlite locking / WAL still allows only one writer at a time'
```

Use explicit fields when adding metadata or calling the command from a script:

```bash
pwf note add app --title "sqlite locking" --content "WAL still allows only one writer at a time" --domain sqlite --tag concurrency --source "SQLite documentation"
```

List, rename or remove notes with their project and note ID:

```bash
pwf note list app
pwf note edit app 1 "sqlite write locking"
pwf note remove app 1
```
