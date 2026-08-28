#!/bin/bash
# EXPECT: PASS. Mutates `revision` on every pane and agent. `revision` indexes the retained
# pane_updated backlog (delta #18) — it is volatile, carries no product meaning, and is on
# normalize.jq's drop list. A red here would mean the drop list is not being applied.
set -uo pipefail
D="${0%/*}"; [ "$D" = "$0" ] && D=.   # pure-bash dirname: `dirname` is unresolvable under the sandbox PATH
. "$D/_fakelib.sh"
require_status "${1:-}"
emit '.result.snapshot.panes[].revision += 1000 | .result.snapshot.agents[].revision += 1000'
