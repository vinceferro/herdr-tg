#!/bin/bash
# EXPECT: PASS. The control. Socket RPC -> `jq -c .`, no mutation at all.
# If this ever fails, the harness (or the herd's own drift) is broken, not the mutation set.
set -uo pipefail
D="${0%/*}"; [ "$D" = "$0" ] && D=.   # pure-bash dirname: `dirname` is unresolvable under the sandbox PATH
. "$D/_fakelib.sh"
require_status "${1:-}"
emit '.'
