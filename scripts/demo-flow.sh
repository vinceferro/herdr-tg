#!/usr/bin/env bash
# demo-flow.sh — drive the Telegram round trip on demand, from a scratch pane.
#
#   bash scripts/demo-flow.sh ask         # an agent asks a question  → answer it from your phone
#   bash scripts/demo-flow.sh permission  # a permission dialog       → tap an option
#   bash scripts/demo-flow.sh done        # an agent finishes         → give it the next thing
#   bash scripts/demo-flow.sh cleanup     # close the scratch tab
#
# WHY THIS EXISTS
#
# The push only fires when an agent is genuinely stuck, which is not something you can arrange while
# you are sitting there wanting to test. So this stages the trigger: it makes a scratch tab, writes
# realistic content into it, and reports that pane's agent status to herdr. The real bridge sees a
# real event on the real socket and does exactly what it would do for a real agent.
#
# WHAT IT WILL NOT DO
#
# It never touches an existing pane. Every flow runs in a tab this script created, labelled
# `herdr-tg demo`, and `cleanup` closes it. Your actual agents are not marked blocked, not typed
# into, and not read. The pane it makes is a plain shell — there is no agent in it, so the only
# thing pretending is the status report.
#
# WHAT TO EXPECT
#
# A push lands ~20s after the report (the debounce). Answer it from Telegram and watch the text
# arrive in the scratch pane — that is the whole round trip, including the read-back that decides
# what the confirmation says.

set -uo pipefail

SOCK="${HERDR_SOCKET_PATH:-$HOME/.config/herdr/herdr.sock}"
LABEL="herdr-tg demo"
STATE="${TMPDIR:-/tmp}/herdr-tg-demo.pane"

rpc() {
  local method="$1" params="$2"
  printf '{"id":"demo","method":"%s","params":%s}\n' "$method" "$params" \
    | socat -t5 - "UNIX-CONNECT:$SOCK" 2>/dev/null
}
jqr() { python3 -c "import json,sys;d=json.load(sys.stdin);print(eval('d'+sys.argv[1]))" "$1" 2>/dev/null; }

command -v socat >/dev/null || { echo "socat is required" >&2; exit 1; }
[ -S "$SOCK" ] || { echo "no herdr socket at $SOCK — is herdr running?" >&2; exit 1; }

# ── the scratch pane ─────────────────────────────────────────────────────────
ensure_pane() {
  if [ -f "$STATE" ]; then
    local existing; existing="$(cat "$STATE")"
    if rpc pane.list '{}' | grep -qF "\"$existing\""; then
      printf '%s' "$existing"; return
    fi
  fi
  local ws; ws="$(rpc workspace.list '{}' | jqr "['result']['workspaces'][0]['workspace_id']")"
  [ -n "$ws" ] || { echo "could not read a workspace from the herd" >&2; exit 1; }
  local tab; tab="$(rpc tab.create "{\"workspace_id\":\"$ws\",\"label\":\"$LABEL\",\"focus\":false}")"
  # `result.root_pane.pane_id` — verified against herdr 0.8.2. There is deliberately NO fallback
  # that picks a pane from the herd: the first version guessed `panes[-1]` and typed into an
  # existing shell in another project. A demo script that cannot identify its own pane must stop,
  # not choose one.
  local pane; pane="$(printf '%s' "$tab" | jqr "['result']['root_pane']['pane_id']")"
  [ -n "$pane" ] || {
    echo "tab.create did not return root_pane.pane_id — refusing to guess which pane is mine." >&2
    printf '%s\n' "$tab" | head -c 400 >&2
    exit 1
  }
  printf '%s' "$pane" > "$STATE"
  printf '%s' "$pane"
}

# Write into the scratch pane. Safe: it is a shell we made, and the text is a comment.
show() {
  local pane="$1" text="$2"
  python3 - "$SOCK" "$pane" "$text" <<'PY'
import json, socket, sys
sock, pane, text = sys.argv[1], sys.argv[2], sys.argv[3]
for line in text.split("\n"):
    s = socket.socket(socket.AF_UNIX); s.connect(sock)
    body = {"id": "demo", "method": "pane.send_input",
            "params": {"pane_id": pane, "text": f"# {line}\n"}}
    s.sendall((json.dumps(body) + "\n").encode()); s.recv(65536); s.close()
PY
}

report() {
  local pane="$1" status="$2"
  rpc pane.report_agent \
    "{\"pane_id\":\"$pane\",\"agent\":\"demo\",\"agent_status\":\"$status\"}" >/dev/null
}

case "${1:-}" in
  ask)
    PANE="$(ensure_pane)"; echo "scratch pane: $PANE"
    show "$PANE" "Rebasing onto main…
Rebase would drop 2 commits from the shipping branch.

Force-push anyway? [y/N]"
    report "$PANE" working; sleep 1; report "$PANE" blocked
    echo "reported BLOCKED. A push should reach your topic in ~20s."
    echo "Reply to it from Telegram, then watch this pane: herdr pane read $PANE --source visible"
    ;;
  permission)
    PANE="$(ensure_pane)"; echo "scratch pane: $PANE"
    show "$PANE" "△ Permission required
  Access external directory ~/.local/share/example

  Allow once   Allow always   Reject      ⇆ select  enter confirm"
    report "$PANE" working; sleep 1; report "$PANE" blocked
    echo "reported BLOCKED with a choice row."
    echo "NOTE: the buttons come from parsing the pane's COLOUR, and a plain shell draws no"
    echo "highlight — so expect the text path, not buttons. Buttons need a real TUI dialog."
    ;;
  done)
    PANE="$(ensure_pane)"; echo "scratch pane: $PANE"
    show "$PANE" "Rebased onto main. 3 files changed, tests green.
Nothing pushed."
    report "$PANE" working; sleep 1; report "$PANE" done
    echo "reported DONE. The topic should show the completion."
    ;;
  cleanup)
    if [ -f "$STATE" ]; then
      PANE="$(cat "$STATE")"
      TAB="$(rpc pane.list '{}' | python3 -c "
import json,sys
p=sys.argv[1]
for x in json.load(sys.stdin)['result']['panes']:
    if x['pane_id']==p: print(x['tab_id']); break
" "$PANE")"
      [ -n "$TAB" ] && rpc tab.close "{\"tab_id\":\"$TAB\"}" >/dev/null && echo "closed tab $TAB"
      rm -f "$STATE"
    else
      echo "nothing to clean up"
    fi
    ;;
  *)
    sed -n '2,28p' "$0"; exit 2 ;;
esac
