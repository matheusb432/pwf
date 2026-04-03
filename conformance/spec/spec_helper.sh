#!/bin/sh
# ShellSpec helpers for the pwf conformance corpus. Stages each fixture's input/
# into a temp dir, runs the release binary with canonical args, and compares
# stdout (JSON subset via jq), stderr (substring), and the output file tree
# (canonical-markdown) against committed goldens. Pure POSIX sh so update-goldens.sh
# can source it too. Mirrors the retired run-conformance.ps1.

cf_root() { printf '%s' "${SHELLSPEC_PROJECT_ROOT:-$PWD}"; }
cf_bin() { printf '%s' "${PWF_BIN:-$(cf_root)/../target/release/pwf}"; }
cf_date() { printf '%s' "${CF_DATE:-2026-01-01}"; }
cf_stub() { printf '%s' "$(cf_root)/pw-stub.sh"; }

# Replace staged absolute paths in engine output with {{REPO}}/{{NOTES}} tokens.
cf_placeholder_text() { # $1=stage   (filters stdin)
  sed -e "s|$1/repo|{{REPO}}|g" -e "s|$1/notes|{{NOTES}}|g"
}

# Expand {{REPO}}/{{NOTES}} tokens in every staged input file to real paths.
cf_expand_placeholders() { # $1=stage
  find "$1" -type f 2>/dev/null | while IFS= read -r f; do
    case $(cat "$f" 2>/dev/null) in
      *'{{REPO}}'* | *'{{NOTES}}'*)
        sed -e "s|{{REPO}}|$1/repo|g" -e "s|{{NOTES}}|$1/notes|g" "$f" >"$f.cftmp" &&
          mv "$f.cftmp" "$f" ;;
    esac
  done
}

# Canonicalize markdown for comparison: sorted frontmatter `key=value`, then a
# whitespace-normalized body. Faithful to ConvertTo-CanonicalMd. (filters stdin)
cf_canonical_md() {
  awk '
    function trim(s){ sub(/^[ \t]+/,"",s); sub(/[ \t]+$/,"",s); return s }
    BEGIN { state=0 }
    state==0 && NR==1 && $0=="---" { state=1; next }
    state==1 {
      if ($0=="---") { state=2; next }
      i=index($0,":")
      if (i>0) { k=substr($0,1,i-1); fm[k]=trim(substr($0,i+1)) }
      next
    }
    { body = body $0 "\n" }
    END {
      n=0; for (k in fm) keys[++n]=k
      for (a=1;a<n;a++) for (b=a+1;b<=n;b++) if (keys[a]>keys[b]) { t=keys[a];keys[a]=keys[b];keys[b]=t }
      printf "==FM==\n"
      for (a=1;a<=n;a++) printf "%s=%s\n", keys[a], fm[keys[a]]
      printf "==BODY==\n"
      gsub(/[ \t]+\n/, "\n", body)
      gsub(/\n\n\n+/, "\n\n", body)
      sub(/^\n+/, "", body); sub(/\n+$/, "", body)
      printf "%s", body
    }
  '
}

# Trailing-whitespace trim for non-markdown files. (filters stdin)
cf_trimend() { awk '{ b=b $0 "\n" } END { sub(/[ \t\n]+$/,"",b); printf "%s", b }'; }

# Compare an expected golden tree against the staged output tree. (echoes diff lines)
cf_compare_tree() { # $1=expected_root $2=actual_root $3=stage
  exp=$1; act=$2; stage=$3
  if [ -d "$exp" ]; then
    find "$exp" -type f ! -name '.gitkeep' 2>/dev/null | while IFS= read -r ef; do
      rel=${ef#"$exp"/}
      af="$act/$rel"
      if [ ! -f "$af" ]; then printf 'tree missing: %s\n' "$rel"; continue; fi
      case $rel in
        *.md)
          e=$(cf_canonical_md <"$ef")
          a=$(cf_placeholder_text "$stage" <"$af" | cf_canonical_md) ;;
        *)
          e=$(cf_trimend <"$ef")
          a=$(cf_placeholder_text "$stage" <"$af" | cf_trimend) ;;
      esac
      [ "$e" = "$a" ] || printf 'tree differs: %s\n' "$rel"
    done
  fi
  if [ -d "$act" ]; then
    find "$act" -type f ! -name '.gitkeep' 2>/dev/null | while IFS= read -r af; do
      rel=${af#"$act"/}
      [ -f "$exp/$rel" ] || printf 'tree unexpected: %s\n' "$rel"
    done
  fi
}

# Stage a fixture, run pwf, return canonical results via globals:
#   CF_EXIT, CF_STDOUT (tokenized), CF_STDERR (tokenized), CF_STAGE.
# Sets the positional params from the fixture + standard args internally.
cf_stage_and_run() { # $1=fixture_dir
  dir=$1
  cmd="$dir/cmd.json"
  CF_ENGINE=$(jq -r '.engine' "$cmd")
  CF_STAGE=$(mktemp -d)
  cp -R "$dir/input/." "$CF_STAGE/" 2>/dev/null || :
  cf_expand_placeholders "$CF_STAGE"

  set --
  while IFS= read -r a; do [ -n "$a" ] && set -- "$@" "$a"; done <<EOF
$(jq -r '.args[]?' "$cmd")
EOF
  cfg="$CF_STAGE/config.json"; date=$(cf_date)
  case $CF_ENGINE in
    pw | pending-work | migrate)
      set -- "$@" --config-path "$cfg" --notes-dir "$CF_STAGE/notes" --date "$date" ;;
    handoff)
      set -- "$@" --config-path "$cfg" --repo-root "$CF_STAGE/repo" --date "$date" \
        --no-commit --pending-work-script "$(cf_stub)" ;;
  esac

  err="$CF_STAGE/stderr.txt"
  CF_STDOUT=$("$(cf_bin)" "$CF_ENGINE" "$@" 2>"$err"); CF_EXIT=$?
  CF_STDOUT=$(printf '%s' "$CF_STDOUT" | cf_placeholder_text "$CF_STAGE")
  CF_STDERR=$(cf_placeholder_text "$CF_STAGE" <"$err" 2>/dev/null || :)
}

# Verify one fixture. Prints one diff line per mismatch; returns 1 if any.
run_fixture() { # $1=fixture_dir
  dir=$1
  cmd="$dir/cmd.json"
  want_exit=$(jq -r '.expectedExitCode // 0' "$cmd")
  cmp_out=$(jq -r '.compareStdout // false' "$cmd")
  cmp_err=$(jq -r '.compareStderr // false' "$cmd")
  cmp_tree=$(jq -r '.compareTree // empty' "$cmd")

  cf_stage_and_run "$dir"
  diffs=""

  [ "$CF_EXIT" = "$want_exit" ] ||
    diffs="${diffs}exit: got $CF_EXIT want $want_exit
"

  if [ "$cmp_out" = "true" ]; then
    exp=$(cat "$dir/expected/stdout.json")
    if ! printf '%s' "$CF_STDOUT" | jq -e --argjson sub "$exp" '. as $w | $sub | inside($w)' >/dev/null 2>&1; then
      diffs="${diffs}stdout: JSON subset mismatch
  want subset: $exp
  got: $CF_STDOUT
"
    fi
  fi

  if [ "$cmp_err" = "true" ]; then
    exp=$(awk '{ b=b $0 "\n" } END { sub(/^[ \t\n]+/,"",b); sub(/[ \t\n]+$/,"",b); printf "%s", b }' "$dir/expected/stderr.txt")
    case $CF_STDERR in
      *"$exp"*) : ;;
      *) diffs="${diffs}stderr: expected to contain '$exp'
" ;;
    esac
  fi

  if [ -n "$cmp_tree" ]; then
    tree_diffs=$(cf_compare_tree "$dir/expected/tree" "$CF_STAGE/$cmp_tree" "$CF_STAGE")
    [ -z "$tree_diffs" ] || diffs="${diffs}${tree_diffs}
"
  fi

  rm -rf "$CF_STAGE"
  [ -z "$diffs" ] && return 0
  printf '%s' "$diffs"
  return 1
}

# Regenerate goldens for one fixture (used by update-goldens.sh).
update_fixture() { # $1=fixture_dir
  dir=$1
  cmd="$dir/cmd.json"
  cmp_out=$(jq -r '.compareStdout // false' "$cmd")
  cmp_err=$(jq -r '.compareStderr // false' "$cmd")
  cmp_tree=$(jq -r '.compareTree // empty' "$cmd")

  cf_stage_and_run "$dir"
  mkdir -p "$dir/expected"
  [ "$cmp_out" = "true" ] && printf '%s' "$CF_STDOUT" >"$dir/expected/stdout.json"
  [ "$cmp_err" = "true" ] && printf '%s' "$CF_STDERR" >"$dir/expected/stderr.txt"
  if [ -n "$cmp_tree" ]; then
    rm -rf "$dir/expected/tree"
    mkdir -p "$dir/expected/tree"
    if [ -d "$CF_STAGE/$cmp_tree" ]; then
      (cd "$CF_STAGE/$cmp_tree" && find . -type f ! -name '.gitkeep') | while IFS= read -r rel; do
        rel=${rel#./}
        mkdir -p "$dir/expected/tree/$(dirname "$rel")"
        cf_placeholder_text "$CF_STAGE" <"$CF_STAGE/$cmp_tree/$rel" >"$dir/expected/tree/$rel"
      done
    fi
  fi
  rm -rf "$CF_STAGE"
  printf 'UPDATED %s\n' "$(basename "$dir")"
}
