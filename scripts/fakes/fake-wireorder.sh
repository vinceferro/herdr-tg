#!/bin/bash
# EXPECT: PASS. Wire key ORDER differs from the reference at every level of the document —
# `jq -S` sorts keys, so canonicalization must absorb this. A client is free to declare its
# structs in any order; that is not a disagreement about the herd.
#
# This file was byte-identical to fake-honest.sh below the shebang in the scratch set (SLICE-1.md
# "Fix before shipping"), so the case had never actually been exercised. It is exercised now:
# every object is re-emitted with its entries reversed, which is a real, verifiable byte
# difference from fake-honest.sh (proof-selftest.sh asserts the two differ before trusting either).
set -uo pipefail
D="${0%/*}"; [ "$D" = "$0" ] && D=.   # pure-bash dirname: `dirname` is unresolvable under the sandbox PATH
. "$D/_fakelib.sh"
require_status "${1:-}"
emit 'walk(if type == "object" then (to_entries | reverse | from_entries) else . end)'
