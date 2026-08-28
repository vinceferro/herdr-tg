#!/bin/bash
# EXPECT: FAIL. Truncates every pane `cwd`. String-field corruption: cwd is how the operator tells
# two identically-named agent panes apart in a Telegram picker, so a client that mangles it routes
# a reply to the wrong terminal — D3's catastrophic failure.
set -uo pipefail
D="${0%/*}"; [ "$D" = "$0" ] && D=.   # pure-bash dirname: `dirname` is unresolvable under the sandbox PATH
. "$D/_fakelib.sh"
require_status "${1:-}"
emit '(.result.snapshot.panes[] | select(.cwd != null) | .cwd) |= .[0:6]'
