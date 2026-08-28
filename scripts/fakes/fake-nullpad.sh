#!/bin/bash
# EXPECT: PASS. Emits explicit `"label": null` where herdr omits the key entirely (delta #19:
# there is not one null anywhere in a live snapshot). normalize.jq drops nulls from BOTH sides,
# so this must not turn the proof red — a missing `skip_serializing_if` is caught by the crate's
# golden round-trip test, not by the live diff, which would flap for a cosmetic reason.
set -uo pipefail
D="${0%/*}"; [ "$D" = "$0" ] && D=.   # pure-bash dirname: `dirname` is unresolvable under the sandbox PATH
. "$D/_fakelib.sh"
require_status "${1:-}"
emit '.result.snapshot.panes[].label = null | .result.snapshot.agents[].label = null'
