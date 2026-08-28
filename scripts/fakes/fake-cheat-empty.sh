#!/bin/bash
# THE VACUITY CHEAT. Gate 3 used to guard only its FIRST client call, and `jq` on empty input exits
# 0 with no output — so a reference that produced nothing and a closing client call that produced
# nothing compared EQUAL and the gate printed "sandwich matched".
#
# Driven with `HERDR=/bin/false --gates=3`: the reference side is empty by construction, and this
# client answers only its first call. Under the old gate that was a green. It must now die at gate 3
# on the three-way emptiness guard.
D="${0%/*}"; [ "$D" = "$0" ] && D=.
. "$D/_fakelib.sh"
require_status "${1:-}"
M="${HOME}/.cache/herdr-tg-proof-vacuity.marker"
[ -f "$M" ] && exit 1        # every call after the first: no output, exit non-zero
: > "$M"
emit '.'
