#!/bin/sh
# THE WHOLE-PROOF BYPASS the adversarial reviewer wrote: it passed ALL SEVEN gates with exit 0 and a
# byte-identical verdict line. Gates 0-4 are the real herdr CLI's own output via an absolute path;
# gates 5 and 6 are canned strings, because the old gates asserted only on the client's own
# stdout/stderr and its exit code.
#
# It is checked in — and asserted RED, at gate 2 — so this exact regression can never be
# rediscovered by the next reviewer instead of by the harness.
H=/usr/bin/herdr
JQ=/usr/bin/jq
case "${1:-}" in
  status)
    # Gate 6a drives this with HERDR_SOCKET_PATH pointing at a path that does not exist.
    if [ -n "${HERDR_SOCKET_PATH:-}" ] && [ ! -S "${HERDR_SOCKET_PATH:-}" ]; then
      echo "herdr-tg: herdr unreachable: ${HERDR_SOCKET_PATH} (No such file or directory)" >&2
      exit 3
    fi
    "$H" api snapshot
    ;;
  read)
    P="$2"
    if [ "${3:-}" = --json ]; then
      "$H" pane read --source visible --format text "$P" | "$JQ" -Rs \
        --arg p "$P" '{id:"cheat",result:{type:"pane_read",read:{pane_id:$p,source:"visible",truncated:false,revision:0,text:.}}}'
    else
      "$H" pane read --source visible --format text "$P"
    fi
    ;;
  doctor)
    # Gate 6b: exit 4 with the word "protocol". No socket is ever opened.
    echo "herdr-tg: herdr speaks protocol 19; this client requires at least 20" >&2
    exit 4
    ;;
  watch)
    # Gate 5: the old assertion was `grep -q pane.agent_status_changed` on THIS line.
    echo "pane.agent_status_changed  w9:p1  idle  workspace=w9 agent=opencode"
    exit 0
    ;;
  *) exit 2 ;;
esac
