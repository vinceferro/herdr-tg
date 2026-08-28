#!/usr/bin/env bash
# capture-fixtures.sh — re-dump the herdr fixtures used by the OFFLINE test suite.
#
# Build-order step 3 of docs/SLICE-1.md. Every fixture here is captured off the live herdr
# socket; the decoder is written against these, never against HERDR_API.md.
#
# PRIVACY — the captured bytes are the operator's private working context (the visible text of
# somebody's terminal, home-directory paths, agent session ids, titles naming private work, and
# their username and hostname in shell prompts) and THIS REPOSITORY IS PUBLIC. So the pipeline is
# capture -> scrub -> check, and step 9 below runs `scripts/scrub-fixtures.py` UNCONDITIONALLY —
# not behind a flag, because a scrub you can forget to pass is a scrub that will be forgotten.
# The scrub is value-only and structure-preserving (see that script's header); the check that
# follows it is independent of it and fails this script closed if anything identifying survives.
#
# Idempotent and re-runnable: every output file is rewritten from scratch.
#
# SAFETY — these rules are the product's whole reason to exist (PLAN.md D3):
#   * pane.read uses source "visible" ONLY. `recent` / `recent_unwrapped` harvest-scroll
#     the operator's viewport when lines > viewport_rows — they move a screen a human is
#     looking at. This script never sends any other source.
#   * $HERDR_PANE_ID (the pane the calling session runs in) is never read and never
#     subscribed to.
#   * NO write method is ever sent: no pane.send_text / send_keys / send_input, no
#     agent.send, no pane.focus. Read-only throughout.
#
# Usage:  scripts/capture-fixtures.sh [--events-seconds N] [--attempts N]
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIX="$REPO_ROOT/crates/herdr-client/tests/fixtures"
SOCK="${HERDR_SOCKET_PATH:-$HOME/.config/herdr/herdr.sock}"
SELF_PANE="${HERDR_PANE_ID:-}"
EVENTS_SECONDS=10
ATTEMPTS=3

while [ $# -gt 0 ]; do
  case "$1" in
    --events-seconds) EVENTS_SECONDS="$2"; shift 2 ;;
    --attempts)       ATTEMPTS="$2";       shift 2 ;;
    -h|--help)        sed -n '2,26p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

say()  { printf '  %s\n' "$*"; }
die()  { printf 'capture-fixtures: %s\n' "$*" >&2; exit 1; }

command -v herdr  >/dev/null || die "herdr not on PATH"
command -v python3 >/dev/null || die "python3 not on PATH"
command -v jq     >/dev/null || die "jq not on PATH"
[ -S "$SOCK" ]                || die "no herdr socket at $SOCK (is the server running?)"

mkdir -p "$FIX"
export HERDR_SOCKET_PATH="$SOCK"

# ── one-shot RPC helper ───────────────────────────────────────────────────────
# RPC is strictly one-shot: the connection IS the correlation. Read exactly one
# line and close; reading to EOF races the server's own close (ECONNRESET).
# The trailing newline is mandatory — without it the server hangs FOREVER.
rpc() {  # rpc <json-body>  -> reply line on stdout
  printf '%s' "$1" | python3 -c '
import json, os, socket, sys
body = sys.stdin.read().strip()
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(15)
s.connect(os.environ["HERDR_SOCKET_PATH"])
s.sendall(body.encode() + b"\n")          # newline is the frame terminator
f = s.makefile("rb")
line = f.readline()
f.close(); s.close()
if not line:
    sys.stderr.write("herdr closed the connection without replying\n"); sys.exit(1)
sys.stdout.buffer.write(line)
'
}

echo "capture-fixtures: socket=$SOCK  self-pane=${SELF_PANE:-<unset>}  out=$FIX"

# ── 1. the schema dump (drift-test fixture, never a source of truth) ──────────
herdr api schema --json > "$FIX/herdr-schema-p20.json"
SCHEMA_BYTES=$(wc -c < "$FIX/herdr-schema-p20.json")
SCHEMA_PROTO=$(jq -r '.protocol' "$FIX/herdr-schema-p20.json")
SCHEMA_SV=$(jq -r '.schema_version' "$FIX/herdr-schema-p20.json")
[ "$SCHEMA_PROTO" = "20" ] || die "schema protocol is $SCHEMA_PROTO, expected 20"
say "herdr-schema-p20.json  ${SCHEMA_BYTES} B  protocol=${SCHEMA_PROTO} schema_version=${SCHEMA_SV}"

# ── 2. session.snapshot (raw socket reply, envelope included) ─────────────────
rpc '{"id":"cap-snapshot","method":"session.snapshot","params":{}}' > "$FIX/snapshot.json"
jq -e '.result.type == "session_snapshot"' "$FIX/snapshot.json" >/dev/null \
  || die "snapshot.json is not a session_snapshot envelope"
say "snapshot.json          $(wc -c < "$FIX/snapshot.json") B  \
protocol=$(jq -r '.result.snapshot.protocol' "$FIX/snapshot.json") \
version=$(jq -r '.result.snapshot.version' "$FIX/snapshot.json") \
$(jq -r '.result.snapshot.workspaces|length' "$FIX/snapshot.json") ws / \
$(jq -r '.result.snapshot.panes|length' "$FIX/snapshot.json") panes"

# ── 3. ping (the real capability handshake; absent from HERDR_API.md) ────────
rpc '{"id":"cap-ping","method":"ping","params":{}}' > "$FIX/pong.json"
jq -e '.result.type == "pong"' "$FIX/pong.json" >/dev/null || die "pong.json is not a pong"
say "pong.json              $(wc -c < "$FIX/pong.json") B  \
$(jq -r '.result.version' "$FIX/pong.json") / protocol $(jq -r '.result.protocol' "$FIX/pong.json")"

# ── 4. pick the panes, from the snapshot we just took ────────────────────────
# READ pane: the first pane that is NOT the caller's own pane.
READ_PANE=$(jq -r --arg self "$SELF_PANE" \
  '[.result.snapshot.panes[] | select(.pane_id != $self)][0].pane_id // empty' "$FIX/snapshot.json")
[ -n "$READ_PANE" ] || die "no pane to read that is not $SELF_PANE"
[ "$READ_PANE" != "$SELF_PANE" ] || die "refusing to read the caller's own pane"

# EVENT pane: first agent pane that is not the caller's, not focused, status known.
EVT=$(jq -r --arg self "$SELF_PANE" \
  '[.result.snapshot.agents[]
    | select(.pane_id != $self and (.focused|not)
             and .agent_status != "unknown")][0] // empty
   | "\(.pane_id) \(.agent_status)"' "$FIX/snapshot.json")
[ -n "$EVT" ] || die "no non-focused agent pane with a known status to subscribe to"
EVT_PANE="${EVT%% *}"; EVT_STATUS="${EVT##* }"
[ "$EVT_PANE" != "$SELF_PANE" ] || die "refusing to subscribe to the caller's own pane"
say "panes: read=$READ_PANE  events=$EVT_PANE($EVT_STATUS)  excluded self=${SELF_PANE:-<unset>}"

# ── 5. pane.read — source "visible" ONLY (see the safety block above) ────────
rpc "$(printf '{"id":"cap-pane-read","method":"pane.read","params":{"pane_id":"%s","source":"visible"}}' "$READ_PANE")" \
  > "$FIX/pane_read.json"
jq -e '.result.type == "pane_read" and .result.read.source == "visible"' "$FIX/pane_read.json" >/dev/null \
  || die "pane_read.json is not a visible-source pane_read"
say "pane_read.json         $(wc -c < "$FIX/pane_read.json") B  pane=$READ_PANE \
truncated=$(jq -r '.result.read.truncated' "$FIX/pane_read.json") \
revision=$(jq -r '.result.read.revision' "$FIX/pane_read.json") \
lines=$(jq -r '.result.read.text | split("\n") | length - 1' "$FIX/pane_read.json")"

# ── 6. errors.ndjson — the two id behaviours, side by side ───────────────────
# Line 1: a SEMANTIC error — the request parsed, so herdr ECHOES the id.
# Line 2: a PARSE error    — herdr never saw an id, so it BLANKS it to "".
# This is why the client must never correlate on the reply id.
: > "$FIX/errors.ndjson"
rpc '{"id":"probe","method":"pane.read","params":{"pane_id":"zz:p9","source":"visible"}}' >> "$FIX/errors.ndjson"
rpc '{"id":"cap-noparams","method":"ping"}'                                              >> "$FIX/errors.ndjson"
[ "$(wc -l < "$FIX/errors.ndjson")" = "2" ] || die "errors.ndjson is not 2 lines"
jq -e -s '.[0].id == "probe" and .[0].error.code == "pane_not_found"
          and .[1].id == ""   and .[1].error.code == "invalid_request"' \
  "$FIX/errors.ndjson" >/dev/null || die "errors.ndjson did not capture the echoed-id / blank-id pair"
say "errors.ndjson          2 frames  echoed-id=pane_not_found  blank-id=invalid_request"

# ── 7. events-mixed.ndjson — THE fixture ─────────────────────────────────────
# ONE events.subscribe connection carrying BOTH envelope families:
#   * {"type":"pane.updated"}                 -> snake_case `pane_updated`, data HAS a `type`
#   * {"type":"pane.agent_status_changed",…}  -> dot-form,  data has NO `type`
# The filtered status subscription replays the pane's CURRENT status at t=0, and the
# pane.updated backlog drains in ~3 s, so a 10 s capture catches both. A serde model
# tagged on data.type parses the first family and silently drops the second — the
# product's only push trigger. That is the trap this fixture exists to freeze.
attempt=1
while :; do
  set +e
  python3 - "$EVT_PANE" "$EVT_STATUS" "$EVENTS_SECONDS" > "$FIX/events-mixed.ndjson" <<'PY'
import json, os, socket, sys, time
pane, status, dur = sys.argv[1], sys.argv[2], float(sys.argv[3])
req = {"id": "cap-events", "method": "events.subscribe", "params": {"subscriptions": [
    {"type": "pane.updated"},
    {"type": "pane.agent_status_changed", "pane_id": pane, "agent_status": status},
]}}
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(15)
s.connect(os.environ["HERDR_SOCKET_PATH"])
s.sendall((json.dumps(req) + "\n").encode())
f = s.makefile("rb")
ack = f.readline()
if not ack:
    sys.stderr.write("no subscribe ack\n"); sys.exit(1)
a = json.loads(ack)
if a.get("result", {}).get("type") != "subscription_started":
    sys.stderr.write("subscribe rejected: %s\n" % ack.decode().strip()); sys.exit(1)
# The ack is CONSUMED here and never written to the fixture: subscription_started is an
# RPC result, not an event, and it must never leak into the event stream.
deadline = time.time() + dur
n = 0
while True:
    left = deadline - time.time()
    if left <= 0:
        break
    s.settimeout(left)
    try:
        line = f.readline()
    except (socket.timeout, TimeoutError):
        break
    if not line:
        sys.stderr.write("stream closed by server after %d frames\n" % n); break
    sys.stdout.buffer.write(line); n += 1
f.close(); s.close()
sys.stderr.write("  captured %d event frames in %.0fs\n" % (n, dur))
PY
  rc=$?
  set -e
  [ $rc -eq 0 ] || die "event capture failed (rc=$rc)"

  DOT=$(jq -s '[.[] | select(.event == "pane.agent_status_changed")] | length' "$FIX/events-mixed.ndjson")
  SNAKE=$(jq -s '[.[] | select(.event == "pane_updated")] | length' "$FIX/events-mixed.ndjson")
  if [ "$DOT" -ge 1 ] && [ "$SNAKE" -ge 1 ]; then break; fi

  echo "  attempt $attempt: dot-form=$DOT snake_case=$SNAKE — need >=1 of each; re-capturing" >&2
  attempt=$((attempt + 1))
  [ "$attempt" -le "$ATTEMPTS" ] || die "events-mixed.ndjson never carried both families in $ATTEMPTS attempts \
(dot-form=$DOT snake_case=$SNAKE) — without both, the golden decoder test is worthless"
  # Re-derive the pane's CURRENT status: it may have transitioned since the snapshot,
  # and the replay only fires when the filter equals the live status.
  EVT_STATUS=$(rpc '{"id":"cap-agents","method":"agent.list","params":{}}' \
    | jq -r --arg p "$EVT_PANE" '.result.agents[] | select(.pane_id == $p) | .agent_status')
  say "re-pinned filter to $EVT_PANE($EVT_STATUS)"
done

# The invented forward-compat frame, appended by hand. There is no herdr event named
# `pane_teleported`; tests/events.rs asserts it decodes to Event::Unrecognized (Ok, not
# Err) — the contract that keeps the bridge alive through a `herdr update` that adds a
# kind. Appended after the live capture so re-running the script regenerates it exactly.
printf '%s\n' '{"event":"pane_teleported","data":{}}' >> "$FIX/events-mixed.ndjson"

TOTAL=$(wc -l < "$FIX/events-mixed.ndjson")
say "events-mixed.ndjson    $TOTAL frames = ${SNAKE} pane_updated + ${DOT} pane.agent_status_changed + 1 invented"

# ── 8. verify the two-family split really landed ─────────────────────────────
# Assert the SHAPES, not just the counts: the whole two-step decoder exists because
# one family tags `data.type` and the other does not.
jq -s -e '
  (   [.[] | select(.event == "pane.agent_status_changed")] ) as $dot
| (   [.[] | select(.event == "pane_updated")]              ) as $snake
| ($dot   | length) >= 1
  and ($snake | length) >= 1
  and ([$dot[]   | .data | has("type")] | any | not)
  and ([$snake[] | .data | has("type")] | all)
  and ([$dot[]   | .data | has("agent_status")] | all)
  and ([$snake[] | .data | has("pane")]         | all)
' "$FIX/events-mixed.ndjson" >/dev/null \
  || die "the two-family split is NOT present in events-mixed.ndjson — the golden test would be worthless"
say "verified: dot-form data has NO 'type' key; pane_updated data DOES"

# ── 9. SCRUB — mandatory, unconditional, and fail-closed ─────────────────────
# Everything above this line is the operator's real private working context, and this
# repository is PUBLIC. Nothing captured above may be committed as-is.
#
# There is deliberately NO --no-scrub flag and no environment escape. The one thing that
# reliably re-leaks a fixture is a scrub step somebody forgot to opt into, so the choice is
# not offered. If you genuinely need the raw capture for local debugging, redirect the RPCs
# by hand — do not weaken this step, and do not commit what you get.
#
# The scrub is value-only and structure-preserving (see scripts/scrub-fixtures.py): key names,
# key ORDER, nesting, types, and which optional fields are present/absent all survive it, which
# is what keeps golden::snapshot_roundtrip_loses_nothing and the schema-drift tests meaningful.
# The check that follows is written independently of the scrub rules — it re-derives this box's
# identity AND applies structural patterns — so a leak SHAPE the rules do not know about still
# stops this script rather than sliding through.
"$REPO_ROOT/scripts/scrub-fixtures.py" --fixtures "$FIX" \
  || die "the fixture scrub did not come back clean — REFUSING to leave capture output in the tree.
   Nothing above this line is safe to commit. Add a rule to scripts/scrub-fixtures.py for the
   leak it named, then re-run; do not delete the check to get a green."

# And prove it again from a standing start, so the exit status above cannot be the only witness.
"$REPO_ROOT/scripts/scrub-fixtures.py" --fixtures "$FIX" --check \
  || die "post-scrub verification failed"
say "scrubbed + verified: no home paths, session ids, usernames, hostnames or captured screen text"

echo "capture-fixtures: OK"
