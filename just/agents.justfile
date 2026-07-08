set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set working-directory := '..'

[private]
_linker := env('HOME') / "agents-shared" / "repo-bootstrap" / "link-claude-skills.ps1"

_require-pwsh:
    @command -v pwsh >/dev/null 2>&1 || { printf '%s\n' "pwsh is required for this legacy bootstrap wrapper." >&2; exit 1; }

# One-time repo setup: link .claude/skills -> .agents/skills.
bootstrap *flags: _require-pwsh
    @[ -f "{{ _linker }}" ] || { printf '%s\n' "deploy the helper first: repository 'just sync-agents-shared'" >&2; exit 1; }
    @pwsh -NoProfile -File "{{ _linker }}" -Repo "$(pwd)" {{ flags }}
