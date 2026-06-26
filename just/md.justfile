# Markdown formatting (mdformat) — synced from config-handler by `cfgtool repos sync-mdformat`.
# Driven off `git ls-files` so gitignored files (vendored skills, plans, build dirs) are never touched.
set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set working-directory := '..'

_run *flags:
    git ls-files --cached --others --exclude-standard -z -- '*.md' \
      | xargs -0 -r uvx --python 3.13 --with mdformat-gfm --with mdformat-gfm-alerts --with mdformat-wikilink --with mdformat-frontmatter mdformat {{ flags }}

# Format all tracked Markdown in place.
fmt: (_run)

# Verify Markdown formatting; non-zero on drift.
fmt-check: (_run "--check")
