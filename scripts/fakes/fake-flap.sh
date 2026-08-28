#!/bin/bash
# EXPECT: PASS *via the sandwich*. Wrong on its FIRST call, honest on every later one.
#
# This is the fake that justifies gate 3's client -> reference -> client shape. `focused` is
# duplicated into every pane and every agent record on top of the three top-level focused_*_id
# fields, so one focus switch between two calls dirties ~22 canonicalized lines. A naive
# single-pair diff would go red for a reason unrelated to client correctness, and an operator who
# is trained to ignore red has no proof at all.
set -uo pipefail
D="${0%/*}"; [ "$D" = "$0" ] && D=.   # pure-bash dirname: `dirname` is unresolvable under the sandbox PATH
. "$D/_fakelib.sh"
require_status "${1:-}"

# Cross-process call counter. $HOME survives `env -i HOME=…`; nothing else does.
CNT="$HOME/.cache/herdr-tg-proof/flap.count"
mkdir -p "$(dirname "$CNT")" 2>/dev/null
N=$(cat "$CNT" 2>/dev/null || echo 0); N=$((N + 1))
printf '%s' "$N" > "$CNT"

if [ "$N" -eq 1 ]; then
  # Stale focus, exactly as if the client had answered from a cache one focus-switch old.
  emit '.result.snapshot.focused_workspace_id = "__stale_focus__"'
else
  rm -f "$CNT"   # self-resetting, so a second selftest run flaps the same way
  emit '.'
fi
