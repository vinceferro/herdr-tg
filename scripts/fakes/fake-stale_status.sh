#!/bin/bash
# EXPECT: FAIL. Flips one pane's agent_status. THE mutation that matters: agent_status is the
# product's entire payload, so normalizing it out would make the proof vacuous. If this ever goes
# green, gate 3 is proving nothing.
set -uo pipefail
D="${0%/*}"; [ "$D" = "$0" ] && D=.   # pure-bash dirname: `dirname` is unresolvable under the sandbox PATH
. "$D/_fakelib.sh"
require_status "${1:-}"
emit '.result.snapshot.panes[0].agent_status |= (if . == "idle" then "working" else "idle" end)'
