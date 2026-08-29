#!/usr/bin/env bash
# install-ask-style.sh — teach a project's agent how to ask a question that survives the trip to a
# phone.
#
# An agent in a herdr pane reads its OWN project's instructions — never this repo's. So the rules in
# docs/ASKING-FROM-A-PHONE.md have to be copied into each project that herdr-tg watches. This script
# is that copy, kept between markers so it is idempotent and removable.
#
#   bash scripts/install-ask-style.sh /path/to/project        # install or refresh
#   bash scripts/install-ask-style.sh --remove /path/to/project
#   bash scripts/install-ask-style.sh --all                   # every project with a live agent pane
#
# Editing a project's agent instructions changes how that agent behaves, so this is deliberately a
# thing the operator runs, not something the bridge does to their repos on its own.

set -euo pipefail

START="<!-- HERDR-TG:ASK-STYLE (install-ask-style.sh) -->"
END="<!-- /HERDR-TG:ASK-STYLE -->"

block() {
  cat <<'BLOCK'
## Asking the operator a question

Your questions reach a human on Telegram, on a phone, possibly hours later. **The bridge relays only
the tail of your rendered screen — about the last 12 non-blank lines.** It cannot see this
conversation, and neither can the person reading.

1. **Ask last.** A question followed by a diff or a table is a question they will not see.
2. **One line of context, then the question.** Assume they have forgotten what this session was for.
3. **Offer closed options and mark the safe one** — `[y/N]`, or a numbered list. A harness
   permission prompt is best: it arrives as buttons they can tap.
4. **Under ten lines.** Longer and the context sentence is cut off the front.
5. **Don't ask mid-render.** Let the spinner or the stream finish first; it occupies the tail.
6. **When you finish, say what you did in a sentence** — completion is relayed too, and the tail of
   a build log tells them nothing.
7. **Never put a secret in a question.** It transits Telegram's servers, as does their answer.

Good:

```
The rebase drops 2 commits from the shipping branch.
Force-push anyway? [y/N]
```

Bad: `Should I proceed?`
BLOCK
}

usage() { sed -n '2,16p' "$0"; exit "${1:-0}"; }

MODE=install
TARGETS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --remove) MODE=remove ;;
    --all)    MODE="${MODE}"; ALL=1 ;;
    -h|--help) usage 0 ;;
    -*) printf 'unknown option: %s\n' "$1" >&2; usage 2 ;;
    *)  TARGETS+=("$1") ;;
  esac
  shift
done

# --all: every project that currently has an agent pane in the herd. The herd is the authority on
# which projects matter; a static list would rot the first time a workspace is added.
if [ "${ALL:-0}" = 1 ]; then
  command -v herdr >/dev/null || { echo "herdr not found, so --all cannot enumerate the herd" >&2; exit 1; }
  mapfile -t TARGETS < <(
    herdr agent list 2>/dev/null \
      | jq -r '.result.agents[] | select(.agent != null) | .cwd' \
      | sort -u
  )
fi

[ ${#TARGETS[@]} -gt 0 ] || usage 2

for dir in "${TARGETS[@]}"; do
  if [ ! -d "$dir" ]; then
    printf '  skip %s (not a directory)\n' "$dir"
    continue
  fi
  # opencode reads AGENTS.md, Claude Code reads CLAUDE.md. Prefer one that already exists so the
  # block lands where the agent is actually looking; default to AGENTS.md for a fresh project.
  file=""
  for cand in AGENTS.md CLAUDE.md; do
    if [ -f "$dir/$cand" ]; then file="$dir/$cand"; break; fi
  done
  [ -n "$file" ] || file="$dir/AGENTS.md"

  # Strip any previous block first, so install is also refresh.
  if [ -f "$file" ] && grep -qF "$START" "$file"; then
    python3 - "$file" "$START" "$END" <<'PY'
import sys, pathlib
p, start, end = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
t = p.read_text()
i, j = t.find(start), t.find(end)
if i != -1 and j != -1:
    p.write_text((t[:i].rstrip() + "\n" + t[j + len(end):].lstrip("\n")).rstrip() + "\n")
PY
  fi

  if [ "$MODE" = remove ]; then
    printf '  removed from %s\n' "$file"
    continue
  fi

  { [ -f "$file" ] && printf '\n'; printf '%s\n' "$START"; block; printf '%s\n' "$END"; } >> "$file"
  printf '  installed in %s\n' "$file"
done

printf '\nAgents pick this up on their next read of the file — usually the next session.\n'
