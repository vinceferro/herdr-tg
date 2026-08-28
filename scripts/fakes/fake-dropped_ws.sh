#!/bin/bash
# EXPECT: FAIL. Drops a workspace. The whole-herd-shape failure: a decoder that silently loses a
# collection would report a smaller herd than exists and the operator would never be told about
# the missing agent's ask.
set -uo pipefail
D="${0%/*}"; [ "$D" = "$0" ] && D=.   # pure-bash dirname: `dirname` is unresolvable under the sandbox PATH
. "$D/_fakelib.sh"
require_status "${1:-}"
emit '.result.snapshot.workspaces |= .[1:]'
