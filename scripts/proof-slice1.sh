#!/usr/bin/env bash
# Slice-1 proof. Seven gates. Exit 0 == slice 1 is done.
#
# ── WHAT AN ADVERSARIAL REVIEWER DID TO THE FIRST VERSION OF THIS FILE ──────────────────────────
# A 30-line `#!/bin/sh` that shelled out to `/usr/bin/herdr` passed all seven gates with exit 0 and
# a byte-identical verdict line. Three holes, all closed below and each one named at its fix:
#   1. the sandbox stripped PATH but not ABSOLUTE-path exec, and herdr lives at a stable path;
#   2. gates 5 and 6 asserted only on the CLIENT'S OWN stdout/stderr — no reference side at all;
#   3. gate 3 compared two possibly-empty strings and called that agreement.
# The rule this file now follows: every gate must assert against something the client does not
# author. Reference output, a byte-compare, a witness socket, a request journal, a per-run nonce.
set -uo pipefail
HERDR="${HERDR:-herdr}"
BIN="${BIN:-$PWD/target/debug/herdr-tg}"
DIR="$(cd "$(dirname "$0")" && pwd)"
NORM="$DIR/normalize.jq"
ATTEMPTS="${ATTEMPTS:-5}"
DROPPED="revision · state_change_seq · scroll · screen_detection_skipped · tokens · terminal_title · terminal_title_stripped · title · state_labels · interactive_ready · launch_pending · layouts · all nulls"
ALL_GATES="0,1,2,3,4,5,6"
ONLY="${GATES:-$ALL_GATES}"

# `--gates=` / `--attempts=` mirror the GATES= / ATTEMPTS= env selectors; proof-selftest.sh drives
# gate 3 in isolation with `--gates=3`, because the mutation fakes are shell scripts and would all
# fail gate 2 by construction — that is the anti-cheat working, not a harness bug.
usage(){ cat >&2 <<EOF
usage: $0 [--gates=0,1,2,3,4,5,6] [--attempts=N]
  env: BIN=<path to herdr-tg>  HERDR=<path to herdr>  GATES=...  ATTEMPTS=N
EOF
}
for arg in "$@"; do
  case "$arg" in
    --gates=*)    ONLY="${arg#--gates=}" ;;
    --attempts=*) ATTEMPTS="${arg#--attempts=}" ;;
    -h|--help)    usage; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$arg" >&2; usage; exit 2 ;;
  esac
done
want(){ case ",$ONLY," in *,"$1",*) return 0;; *) return 1;; esac; }
ok(){   printf 'gate %-2s %-22s ok    %s\n' "$1" "$2" "${3:-}"; }
die(){  printf 'gate %-2s %-22s FAIL  %s\n' "$1" "$2" "$3" >&2; exit 1; }
EMPTY="$(mktemp -d)"; TMP="$(mktemp -d)"; trap 'rm -rf "$EMPTY" "$TMP"' EXIT
nonce(){ head -c 12 /dev/urandom | od -An -tx1 | tr -d ' \n'; }

# ── THE SANDBOX ─────────────────────────────────────────────────────────────────────────────────
# Two independent layers, because the first version had only the weaker half of one of them.
#
# (a) `$BIN` must be an ELF executable. Kills every shell stand-in outright — a proof that a Rust
#     client speaks the wire cannot be satisfied by a script, whatever the script prints. Applied
#     in gate 2 only, so proof-selftest.sh's `--gates=3` mutation fakes (deliberately shell) still
#     drive the gate whose discrimination they exist to test.
#
# (b) The herdr CLI is NEUTERED, not merely hidden. `env -i PATH=<empty>` only stops bare-name
#     resolution; `/usr/bin/herdr` still executes and defeated the whole proof. So the client runs
#     inside an unprivileged user+mount namespace (`unshare -rm`) with a `exit 127` decoy
#     bind-mounted over EVERY herdr binary on this box — discovered at run time, not hard-coded, so
#     a second copy appearing tomorrow is covered the day it appears. Nothing outside this
#     process's namespace is touched: the mounts die with the child.
#
# And gate 2 PROVES ITS OWN SANDBOX every run (`sandbox self-check` below) rather than assuming it:
# it execs the real herdr CLI through the same wrapper and requires exit 127. A sandbox that
# silently stopped being applied is exactly how the first version came to print green for a cheat.
#
# Residual, stated rather than hidden: a client that ships its OWN copy of the herdr CLI inside
# itself, or that reimplements `herdr api snapshot` by dialling the socket, is not distinguishable
# from a real client here — and the second one is not a cheat, it is the product.
DECOY="$TMP/herdr-cli-decoy"; printf '#!/bin/sh\nexit 127\n' > "$DECOY"; chmod 0755 "$DECOY"
NEUTER="$TMP/neuter.sh"
SANDBOX_READY=0

herdr_clis(){
  { type -aP herdr 2>/dev/null
    command -v "$HERDR" 2>/dev/null
    [ -n "${HERDR_BIN_PATH:-}" ] && printf '%s\n' "$HERDR_BIN_PATH"
    for d in /usr/bin /usr/local/bin /bin /sbin /usr/sbin /opt/herdr/bin \
             "$HOME/.local/bin" "$HOME/.cargo/bin" "$HOME/bin"; do
      [ -x "$d/herdr" ] && printf '%s\n' "$d/herdr"
    done
  } 2>/dev/null | while IFS= read -r p; do
      [ -f "$p" ] || continue
      printf '%s\n' "$p"; readlink -f "$p"
    done | sort -u
}

# Build the in-namespace wrapper, then assert the namespace really neuters the CLI.
ensure_sandbox(){
  local g="$1"
  [ "$SANDBOX_READY" = 1 ] && return 0
  command -v unshare >/dev/null 2>&1 \
    || die "$g" "sandbox self-check" "unshare(1) is not installed — the herdr CLI cannot be neutered and no green here would mean anything"
  unshare -rm true >/dev/null 2>&1 \
    || die "$g" "sandbox self-check" "unprivileged user namespaces are unavailable — the herdr CLI cannot be neutered and no green here would mean anything"

  herdr_clis > "$TMP/herdr-clis.txt"
  [ -s "$TMP/herdr-clis.txt" ] \
    || die "$g" "sandbox self-check" "found no herdr CLI to neuter — the discovery list is broken, which is how a shell-out cheat gets in"

  { echo '# Generated by proof-slice1.sh. Runs INSIDE `unshare -rm` (fresh user + mount namespace).'
    echo 'set -e'
    while IFS= read -r p; do printf 'mount --bind "$DECOY" %q\n' "$p"; done < "$TMP/herdr-clis.txt"
    cat <<'INNER'
if [ -n "${CLIENT_SOCKET:-}" ]; then
  exec env -i HOME="$CLIENT_HOME" PATH="$CLIENT_PATH" HERDR_SOCKET_PATH="$CLIENT_SOCKET" "$CLIENT_BIN" "$@"
else
  exec env -i HOME="$CLIENT_HOME" PATH="$CLIENT_PATH" "$CLIENT_BIN" "$@"
fi
INNER
  } > "$NEUTER"
  SANDBOX_READY=1

  # The self-check: every discovered herdr CLI, executed through the very wrapper the client will
  # be run through, must be dead. `api snapshot` is the exact call the reviewer's cheat made.
  #
  # Wrapped in `timeout` because a decoy that is broken rather than absent can HANG rather than
  # exit: mutation-testing this file with a pass-through decoy hung the self-check indefinitely
  # instead of failing it, and a proof that hangs is a proof nobody reads the verdict of. rc 124 is
  # not 127, so a hang is a FAIL like any other.
  local p rc out
  local TO=""; command -v timeout >/dev/null 2>&1 && TO="timeout 15"
  while IFS= read -r p; do
    out="$($TO env DECOY="$DECOY" CLIENT_BIN="$p" CLIENT_HOME="$HOME" CLIENT_PATH="$EMPTY" \
             CLIENT_SOCKET="" unshare -rm sh "$NEUTER" api snapshot 2>/dev/null)"; rc=$?
    [ "$rc" = 127 ] || die "$g" "sandbox self-check" \
      "$p still ran inside the namespace (exit $rc, ${#out} B of output) — the decoy bind-mount is NOT in force and a shell-out would pass every gate"
  done < "$TMP/herdr-clis.txt"
}

# Run any binary the way the client is run: no PATH, no HERDR_* env unless SOCK_OVERRIDE says so,
# and every herdr CLI on the box replaced by `exit 127`.
sandboxed(){
  local b="$1"; shift
  DECOY="$DECOY" CLIENT_BIN="$b" CLIENT_HOME="$HOME" CLIENT_PATH="$EMPTY" \
    CLIENT_SOCKET="${SOCK_OVERRIDE:-}" unshare -rm sh "$NEUTER" "$@"
}
# Fail LOUDLY rather than silently producing nothing: while building this file, a gate that used
# `client` without calling `ensure_sandbox` first ran `sh <nonexistent wrapper>` and returned empty
# output, which gate 3 then reported as a comparison failure rather than as a harness bug.
client(){
  [ "$SANDBOX_READY" = 1 ] || { printf 'HARNESS BUG: client() called before ensure_sandbox\n' >&2; exit 2; }
  sandboxed "$BIN" "$@"
}

# Gates 4/5 need the reference snapshot; fetch it if gate 0 was not selected.
need_ref(){
  [ -n "${R0:-}" ] && return 0
  R0="$($HERDR api snapshot 2>&1)" || die "$1" "reference" "\`$HERDR api snapshot\` exited non-zero: $(head -1 <<<"$R0")"
  [ "$(jq -r '.result.type // "none"' <<<"$R0")" = session_snapshot ] || die "$1" "reference" "reference is not a session_snapshot envelope"
}

# ---- gate 0: reference sanity -------------------------------------------------
if want 0; then
  R0="$($HERDR api snapshot 2>&1)" || die 0 "reference sane" "\`$HERDR api snapshot\` exited non-zero: $R0"
  [ "$(jq -r '.result.type // "none"' <<<"$R0")" = session_snapshot ] || die 0 "reference sane" "not a session_snapshot envelope"
  PROTO="$(jq -r '.result.snapshot.protocol' <<<"$R0")"
  [ "$PROTO" = 20 ] || die 0 "reference sane" "herdr speaks protocol $PROTO, this client is pinned to 20 — refresh the schema fixture and re-read KNOWN_PROTOCOL before trusting any later gate"
  ok 0 "reference sane" "session_snapshot, protocol 20, herdr $(jq -r .result.snapshot.version <<<"$R0")"
fi

# ---- gate 1: non-vacuity ------------------------------------------------------
if want 1; then
  need_ref 1
  NW=$(jq '.result.snapshot.workspaces|length' <<<"$R0"); NP=$(jq '.result.snapshot.panes|length' <<<"$R0")
  { [ "$NW" -gt 0 ] && [ "$NP" -gt 0 ]; } || die 1 "herd non-empty" "herd is empty — the proof would be vacuous"
  ok 1 "herd non-empty" "$NW workspaces / $NP panes"
fi

# ---- gate 2: the client is a real client, not a herdr-CLI wrapper -------------
if want 2; then
  [ -f "$BIN" ] && [ -x "$BIN" ] || die 2 "sandboxed client" "\$BIN is not an executable file: $BIN"
  # A shell stand-in dies here whatever it prints. `\x7fELF` is the magic; target/debug/herdr-tg
  # matches, every `#!/bin/…` script does not.
  head -c 4 "$BIN" | grep -qa $'^\x7fELF' \
    || die 2 "sandboxed client" "\$BIN is not an ELF executable (magic: $(head -c 4 "$BIN" | od -An -c | tr -s ' ')) — a script cannot prove a Rust client speaks the wire"
  ensure_sandbox 2
  NCLI=$(wc -l < "$TMP/herdr-clis.txt")
  S="$(client status --json 2>"$TMP/e2")" || die 2 "sandboxed client" "exit $? under the neutered sandbox: $(head -1 "$TMP/e2")"
  [ "$(jq -r '.result.type // "none"' <<<"$S")" = session_snapshot ] || die 2 "sandboxed client" "no session_snapshot envelope under a stripped env"
  ok 2 "sandboxed client" "ELF, PATH=<empty>, no HERDR_* env, $NCLI herdr CLI path(s) bind-mounted to \`exit 127\` → socket fallback works"
fi

# ---- gate 3: sandwiched canonical equivalence ---------------------------------
if want 3; then
  # Gate 3 runs the client too, so it runs it under the same neutered sandbox. (This is also what
  # makes `--gates=3` in isolation meaningful: the mutation fakes reach the socket with socat, which
  # is legitimate — a stand-in needs it and the real client needs no PATH at all — but the herdr CLI
  # itself is dead here as well, so a gate-3-only shell-out cannot pass either.)
  ensure_sandbox 3
  norm(){ jq -S -f "$NORM"; }
  for i in $(seq 1 "$ATTEMPTS"); do
    A="$(client status --json 2>/dev/null | norm)"
    R="$($HERDR api snapshot | norm)"
    C="$(client status --json 2>/dev/null | norm)"
    # ALL THREE, not just $A: `jq` on empty input exits 0 with no output, so an unguarded
    # comparison of two empty strings reports "sandwich matched" and proves nothing.
    { [ -n "$A" ] && [ -n "$R" ] && [ -n "$C" ]; } \
      || die 3 "snapshot equivalence" "empty side (client-open=${#A} reference=${#R} client-close=${#C} bytes) — a blank comparison is not a match"
    if [ "$R" = "$A" ] || [ "$R" = "$C" ]; then ok 3 "snapshot equivalence" "sandwich matched on attempt $i"; MATCHED=1; break; fi
    LR="$R"; LA="$A"
  done
  if [ "${MATCHED:-0}" != 1 ]; then
    { echo "--- diff: herdr api snapshot (<) vs herdr-tg status --json (>) ---"; diff <(printf '%s' "$LR") <(printf '%s' "$LA")
      echo "--- normalized out of BOTH sides: $DROPPED ---"; } >&2
    die 3 "snapshot equivalence" "client disagrees with herdr on all $ATTEMPTS attempts"
  fi
fi

# ---- gate 4: pane.read parity -------------------------------------------------
if want 4; then
  need_ref 4
  ensure_sandbox 4
  # Hard safety rule: never read the pane this session runs in. Fail CLOSED — an unset
  # HERDR_PANE_ID used to degenerate to `select(.pane_id != "")`, which excludes nothing.
  ME="${HERDR_PANE_ID:-}"
  [ -n "$ME" ] || die 4 "pane.read parity" "HERDR_PANE_ID is unset; refusing to pick a pane to read because it could be the operator's own (export it, or set it to a non-existent id to opt out deliberately)"
  P="$(jq -r --arg me "$ME" '[.result.snapshot.panes[]|select(.pane_id != $me)][0].pane_id' <<<"$R0")"
  { [ -n "$P" ] && [ "$P" != null ] && [ "$P" != "$ME" ]; } || die 4 "pane.read parity" "no pane to read other than this session's own ($ME)"
  VR="$(jq -r --arg p "$P" '.result.snapshot.panes[]|select(.pane_id==$p)|.scroll.viewport_rows' <<<"$R0")"
  for i in 1 2 3; do
    "$HERDR" pane read --source visible --format text "$P" > "$TMP/ref.txt" 2>/dev/null
    client read "$P" > "$TMP/cli.txt" 2>/dev/null
    cmp -s "$TMP/ref.txt" "$TMP/cli.txt" && { PAR=1; break; }
  done
  [ "${PAR:-0}" = 1 ] || { diff "$TMP/ref.txt" "$TMP/cli.txt" | head -20 >&2; die 4 "pane.read parity" "text differs from \`herdr pane read --source visible\` on $P after 3 attempts"; }
  J="$(client read "$P" --json 2>/dev/null)"
  [ "$(jq -r '.result.read.source' <<<"$J")" = visible ] || die 4 "pane.read parity" "client did not send source=visible (recent would scroll the operator's screen)"
  [ "$(jq -r '.result.read.truncated' <<<"$J")" = false ] || die 4 "pane.read parity" "client reported truncated=true on a full viewport read"
  # `viewport_rows` is LIVE and can change under a proof run — observed dropping 63 -> 61 -> 63 while
  # this gate was executing, which failed the bound against the value gate 0 happened to capture and
  # turned a healthy client into a red proof. So the bound is re-read next to the parity read and
  # either reading is accepted: both are legitimate reference values for a pane that resized
  # mid-run. This does not soften what the bound is for — a `recent` scroll harvest returns
  # hundreds of lines, not one more than the viewport — and `cmp` against
  # `herdr pane read --source visible` remains the gate's primary assertion.
  VRNOW="$("$HERDR" api snapshot 2>/dev/null | jq -r --arg p "$P" '.result.snapshot.panes[]|select(.pane_id==$p)|.scroll.viewport_rows')"
  case "$VRNOW" in ''|null) VRNOW="$VR" ;; esac
  L=$(wc -l < "$TMP/cli.txt")
  { [ "$L" -gt 0 ] && { [ "$L" -le "$VRNOW" ] || [ "$L" -le "$VR" ]; }; } \
    || die 4 "pane.read parity" "$L lines is outside 1..$VRNOW (viewport_rows now; it read $VR at gate 0)"
  ok 4 "pane.read parity" "$P: $(wc -c < "$TMP/cli.txt") B byte-identical, truncated=false, $L/$VRNOW rows"
fi

# ---- gate 5a: the two-envelope decoder, against an INDEPENDENT WITNESS ---------
# The old gate 5 was `grep -q pane.agent_status_changed <<< "$OUT"` on the client's own stdout plus
# exit 0 — a client that printed the string and never opened a socket passed, and the success line
# interpolated the pane/status from the REFERENCE snapshot, so the harness printed a convincing line
# whatever the client said.
#
# Now the frame comes from scripts/mock-herdr.py on a scratch socket and carries a SENTINEL pane,
# workspace and a per-run NONCE that exist nowhere in the live herd, preceded by a snake_case
# `pane_updated` decoy carrying the same status as a HISTORICAL value. Both directions are asserted:
#   • the mock's journal proves the client dialled and sent a correctly-shaped events.subscribe;
#   • the client's rendered line must carry the nonce, which only that frame could have supplied.
# A client that cannot decode has nothing to print; a client that prints a canned string prints the
# wrong sentinel; a client that mistakes the roster family for the ask matches the decoy and never
# reaches the real frame.
#
# WHAT 5a DOES NOT PROVE, measured rather than assumed: a shell script that dials the socket with
# socat, sends a correct ping + events.subscribe, and reformats the frame with jq DOES pass this
# gate — verified. That is the right answer: it really did read the frame off the wire and render
# its fields, which is what 5a asks. What rules it out as a herdr-tg proof is gate 2 (not an ELF,
# and every herdr CLI neutered). Neither gate is the property on its own; the pair is.
#
# The nonce is fed to the mock on STDIN, never argv — see the spawn below.
if want 5; then
  need_ref 5
  SP_PANE="zz:p0"; SP_WS="zzsentinel"; SP_STATUS="blocked"; SP_NONCE="$(nonce)"
  jq -e --arg p "$SP_PANE" '[.result.snapshot.panes[].pane_id]|index($p)|not' <<<"$R0" >/dev/null \
    || die 5a "event decode" "$SP_PANE exists in the live herd — it is not a sentinel any more; pick another"
  ensure_sandbox 5a
  JRNL="$TMP/ev.journal"; : > "$JRNL"
  # The nonce goes in on STDIN, never on the mock's command line: /proc/<pid>/cmdline is
  # world-readable and `unshare -rm` is not a PID namespace, so an argv nonce can be scraped by the
  # client under test and echoed back with no socket ever opened. Verified: that attack passed the
  # rendered-line assertion before this line was written.
  python3 "$DIR/mock-herdr.py" --socket "$TMP/ev.sock" --protocol 20 --version 0.8.2 --requests 2 \
    --events --journal "$JRNL" --sentinel-pane "$SP_PANE" --sentinel-workspace "$SP_WS" \
    --sentinel-status "$SP_STATUS" --sentinel-agent - <<<"$SP_NONCE" >/dev/null 2>&1 &
  EPID=$!; for _ in $(seq 1 60); do [ -S "$TMP/ev.sock" ] && break; sleep 0.05; done
  [ -S "$TMP/ev.sock" ] || { kill $EPID 2>/dev/null; die 5a "event decode" "the witness mock never bound $TMP/ev.sock"; }
  OUT5A="$(SOCK_OVERRIDE="$TMP/ev.sock" client watch --once --pane "$SP_PANE" --expect-status "$SP_STATUS" --timeout-ms 5000 2>"$TMP/e5a")"; RC5A=$?
  kill $EPID 2>/dev/null; wait $EPID 2>/dev/null

  [ "$RC5A" = 0 ] || die 5a "event decode" "client exited $RC5A against the witness mock: $(head -1 "$TMP/e5a")"
  # The rendered line, in full. Every field on it came off the witness socket and nowhere else.
  WANT5="pane.agent_status_changed  $SP_PANE  $SP_STATUS  workspace=$SP_WS agent=$SP_NONCE"
  [ "$OUT5A" = "$WANT5" ] || die 5a "event decode" "client did not render the witness frame.
    expected: $WANT5
    got     : $OUT5A"
  # ...and the mock's own record that it was contacted, with the right request.
  [ "$(grep -c '"method": "events.subscribe"' "$JRNL")" = 1 ] \
    || die 5a "event decode" "the witness mock received $(grep -c '"method"' "$JRNL") request(s) but not exactly one events.subscribe — the client did not open the stream"
  SUBJ="$(jq -c 'select(.method=="events.subscribe")|.params.subscriptions[0]' "$JRNL")"
  [ "$(jq -r '.type' <<<"$SUBJ")"         = "pane.agent_status_changed" ] || die 5a "event decode" "subscribed to the wrong variant: $SUBJ"
  [ "$(jq -r '.pane_id' <<<"$SUBJ")"      = "$SP_PANE" ]                  || die 5a "event decode" "subscribed to the wrong pane: $SUBJ"
  [ "$(jq -r '.agent_status' <<<"$SUBJ")" = "$SP_STATUS" ]                || die 5a "event decode" "subscribed without the status filter that makes the replay deterministic: $SUBJ"
  grep -q '"method": "ping"' "$JRNL" || die 5a "event decode" "the client never handshook before subscribing"
  ok 5a "event decode" "witness socket: sentinel $SP_PANE{$SP_STATUS} agent=$SP_NONCE rendered; roster decoy skipped; subscribe journalled"

  # ---- gate 5b: the same decode against the LIVE herd (replay at t=0) ----------
  # What the mock cannot prove: that a filtered subscription against a REAL pane replays the
  # current status immediately, with no transition and nothing typed anywhere. That property is
  # slice 3's "laptop was asleep, recover the missed ask" path.
  #
  # Same fail-closed self-pane exclusion as gate 4, and it was missing here: an events.subscribe
  # against the pane this session runs in is read-only, but "the proof opened a stream on the
  # operator's own pane" is not a sentence this file should ever be able to produce, and the
  # in-loop re-selection could drift onto it on attempt 2 or 3 even when attempt 1 was safe.
  ME_5="${HERDR_PANE_ID:-}"
  [ -n "$ME_5" ] || die 5b "live replay at t=0" "HERDR_PANE_ID is unset; refusing to pick a pane to subscribe to because it could be the operator's own (export it, or set it to a non-existent id to opt out deliberately)"
  read -r EP ES < <(jq -r --arg me "$ME_5" '[.result.snapshot.agents[]|select(.agent_status!="unknown" and .pane_id!=$me)][0]|"\(.pane_id) \(.agent_status)"' <<<"$R0")
  for i in 1 2 3; do
    OUT="$(client watch --once --pane "$EP" --expect-status "$ES" --timeout-ms 5000 2>"$TMP/e5")" && { EV=1; break; }
    R0="$($HERDR api snapshot)"
    read -r EP ES < <(jq -r --arg me "$ME_5" '[.result.snapshot.agents[]|select(.agent_status!="unknown" and .pane_id!=$me)][0]|"\(.pane_id) \(.agent_status)"' <<<"$R0")
  done
  [ "${EV:-0}" = 1 ] || die 5b "live replay at t=0" "no pane.agent_status_changed decoded for $EP in 3 attempts: $(head -1 "$TMP/e5")"
  # Anchored: an unanchored `pane.agent_status_changed` also matches the string
  # `unrecognized:pane_agent_status_changed`, which is what render::event_line prints if the
  # decoder ever regresses to bucketing the dot form.
  grep -q "^pane\.agent_status_changed  $EP  $ES  workspace=" <<<"$OUT" || die 5b "live replay at t=0" "decoded something else: $OUT"
  ok 5b "live replay at t=0" "$EP → $(head -1 <<<"$OUT")"
fi

# ---- gate 6: failure paths ----------------------------------------------------
# Both cases now carry a witness. 6a: the socket path is a per-run sentinel, so the error text has
# to be derived rather than canned. 6b: the mock journals the request to an unguessable path in an
# unguessable directory that is never handed to the client, so "the client actually
# contacted a protocol-19 server" is asserted rather than inferred from its own exit code.
if want 6; then
  ensure_sandbox 6a
  S6N="$(nonce)"; S6="$TMP/absent-$S6N/herdr-$S6N.sock"
  SOCK_OVERRIDE="$S6" client status >"$TMP/o6" 2>"$TMP/e6"; RC=$?
  [ "$RC" = 3 ] || die 6a "failure paths" "missing socket exited $RC, expected 3"
  grep -qi "herdr unreachable" "$TMP/e6" || die 6a "failure paths" "missing socket did not print 'herdr unreachable'"
  grep -qF "$S6" "$TMP/e6" || die 6a "failure paths" "the error names no socket path, or a different one than the client was given — it cannot have come from the failed dial: $(head -1 "$TMP/e6")"
  grep -q "panicked at" "$TMP/e6" && die 6a "failure paths" "client PANICKED on a missing socket"
  ok 6a "failure paths" "missing socket → exit 3, error names the per-run sentinel path, no panic"

  # The journal must live where the CLIENT CANNOT REACH IT. Round-two review forged this witness:
  # the journal used to sit at "$TMP/skew.journal", beside the socket the client is handed, so a
  # cheat derived the path with ${HERDR_SOCKET_PATH%/*} and appended the ping line itself — passing
  # 6b, and making the gate PRINT "mock journalled the ping", without ever dialling. That is the
  # client authoring its own evidence, which line 11 of this file forbids. Unguessable directory,
  # unguessable filename, never exported to the client.
  W6="$(mktemp -d "${TMPDIR:-/tmp}/hproof-witness-XXXXXXXX")"
  J6="$W6/$(head -c 12 /dev/urandom | od -An -tx1 | tr -d ' \n').journal"; : > "$J6"
  python3 "$DIR/mock-herdr.py" --socket "$TMP/skew.sock" --protocol 19 --requests 2 --journal "$J6" >/dev/null 2>&1 &
  MPID=$!; for _ in $(seq 1 60); do [ -S "$TMP/skew.sock" ] && break; sleep 0.05; done
  [ -S "$TMP/skew.sock" ] || { kill $MPID 2>/dev/null; die 6b "failure paths" "the protocol-19 mock never bound its socket"; }
  SOCK_OVERRIDE="$TMP/skew.sock" client doctor >"$TMP/o7" 2>"$TMP/e7"; RC=$?
  kill $MPID 2>/dev/null; wait $MPID 2>/dev/null
  [ "$RC" = 4 ] || die 6b "failure paths" "protocol 19 exited $RC, expected 4"
  grep -qi "protocol" "$TMP/e7" || die 6b "failure paths" "protocol skew message did not mention the protocol"
  grep -q '"method": "ping"' "$J6" || die 6b "failure paths" "the protocol-19 mock was never contacted — the client fabricated its skew verdict without opening the socket"
  rm -rf "$W6"
  ok 6b "failure paths" "protocol 19 → exit 4 (mock journalled the ping at a path the client is never given), no panic"
fi

# A subset run must NOT print the full verdict: proof-selftest.sh calls this with --gates=3 many
# times over passing fakes, and "SLICE 1 PROOF: PASS" from a one-gate run would be a lie.
if [ "$ONLY" = "$ALL_GATES" ]; then
  echo "SLICE 1 PROOF: PASS — herdr-tg agrees with herdr 0.8.2 / protocol 20 on ${NW:-?} workspaces / ${NP:-?} panes"
else
  echo "gates $ONLY passed — SUBSET RUN, not the full slice-1 proof"
fi
