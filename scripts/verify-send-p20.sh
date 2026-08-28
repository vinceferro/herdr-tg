#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────────────────────────
# verify-send-p20.sh — settle the send path against a THROWAWAY `herdr --session probe`.
#
# DEFERRED BY DESIGN. Slice 1 ships pane.send_text / pane.send_keys / pane.send_input as typed,
# mock-tested Rust with no live call site and no binary subcommand that reaches them, because
# proving them means typing real keystrokes into a real terminal — the exact catastrophic failure
# D3 exists to prevent. This script is written now, while the reasoning is fresh, and RUN LATER, by
# the operator, at the machine, before slice 3 (SLICE-1.md build order step 15).
#
# It settles the five things the spec is careful not to lean on:
#   P1  the success TAG of the three send methods (`ok` is inferred, never observed)
#   P2  the pane.send_keys KEY GRAMMAR on protocol 20 (p16 evidence is not a p20 fact; the schema
#       does not constrain keys at all, and a bogus pane returns pane_not_found before invalid_key,
#       so this is unprobeable without a real pane)
#   P3  whether pane.send_input frames its text in BRACKETED PASTE — multi-line Telegram replies
#       are this product's default case, and HERDR_API.md's 0.7.4 finding is that send_text writes
#       RAW bytes, i.e. a \n inside text is a real Enter that executes a line in the operator's shell
#   P4  whether a FILTERED subscription fires on later transitions INTO the filtered status, or is
#       catch-up-at-subscribe only (slice 3's whole recovery path turns on this)
#   P5  whether state_change_seq CHURNS while a pane sits blocked (the dedupe key's soundness)
#
# ── THE REFUSAL ─────────────────────────────────────────────────────────────────────────────────
# It refuses to run unless BOTH hold:
#   * HERDR_TG_PROBE_SESSION is set, and
#   * the resolved socket is NOT the operator's live socket (checked by realpath AND by device:inode,
#     against ~/.config/herdr/herdr.sock and against $HERDR_SOCKET_PATH).
# Plus four more refusals that cost nothing and close the ways a mistake actually happens: the
# target pane may not be $HERDR_PANE_ID, must exist in the PROBE snapshot, must have NO detected
# agent (a plain shell only), and the probe session must be small (a real herd is not).
#
# Every RPC in this script goes over the probe socket with socat. The herdr CLI is invoked at most
# once, only to RESOLVE the probe socket path, and its answer is then put through the full refusal
# gate. Set HERDR_TG_PROBE_SOCKET to skip even that.
#
# ── SETUP (the operator, at the machine) ────────────────────────────────────────────────────────
#   1. herdr --session probe            # a second, throwaway session; does not touch the live one
#   2. inside it, open a plain shell pane and leave it at a prompt (no agent)
#   3. from anywhere:
#        HERDR_TG_PROBE_SESSION=probe ./scripts/verify-send-p20.sh --dry-run   # gates only, sends nothing
#        HERDR_TG_PROBE_SESSION=probe ./scripts/verify-send-p20.sh             # the real run
#   4. when done: exit the probe session. Nothing here writes to disk except the audit log.
#
# Env:
#   HERDR_TG_PROBE_SESSION   (required) name of the throwaway session
#   HERDR_TG_PROBE_SOCKET    (optional) probe socket path; skips CLI resolution entirely
#   HERDR_TG_PROBE_PANE      (optional) target pane id; default: the only agent-less pane
#   HERDR_TG_PROBE_MAX_PANES (optional) refuse above this pane count (default 4)
# ─────────────────────────────────────────────────────────────────────────────────────────────────
set -uo pipefail

DRY=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY=1 ;;
    -h|--help) sed -n '2,60p' "$0"; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$arg" >&2; exit 2 ;;
  esac
done

HERDR="${HERDR:-herdr}"
LIVE_DEFAULT="$HOME/.config/herdr/herdr.sock"
MAXPANES="${HERDR_TG_PROBE_MAX_PANES:-4}"
TS="$(date +%Y%m%dT%H%M%S)"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT

# The audit log is written to $TMP until the refusal gate has PASSED, then moved to its real home.
# A refused run must leave nothing behind: the log used to be created (and `: > "$AUDIT"`-truncated)
# before the gate ran, so every correctly-refused invocation dropped a stray
# verify-send-p20.<ts>.audit.log in the repo root. It is git-ignored, so it was litter rather than a
# leak, but it is litter in a tree that gets committed by hand. $TMP is removed by the EXIT trap, so
# the refusal path now cleans up after itself; the move below is what makes the log survive.
AUDIT_FINAL="${AUDIT_LOG:-$PWD/verify-send-p20.$TS.audit.log}"   # *.audit.log is git-ignored by design
AUDIT="$TMP/audit.log"

refuse(){ printf '\nREFUSING TO RUN: %s\n' "$1" >&2; exit 2; }
say(){ printf '%s\n' "$*"; printf '%s\n' "$*" >> "$AUDIT"; }
hdr(){ printf '\n\033[1m%s\033[0m\n' "$*"; printf '\n== %s\n' "$*" >> "$AUDIT"; }

command -v socat >/dev/null || refuse "socat is not on PATH; every RPC here goes over it"
command -v jq    >/dev/null || refuse "jq is not on PATH"

: > "$AUDIT"
say "verify-send-p20 $TS  (dry-run=$DRY)"

# ══ REFUSAL GATE ═════════════════════════════════════════════════════════════════════════════════
S="${HERDR_TG_PROBE_SESSION:-}"
[ -n "$S" ] || refuse "HERDR_TG_PROBE_SESSION is not set.
  This script types real keystrokes into a real terminal. It runs ONLY against a throwaway
  \`herdr --session <name>\` you started for the purpose. Start one, then re-run with
  HERDR_TG_PROBE_SESSION=<name>."

case "$S" in
  default|"") refuse "HERDR_TG_PROBE_SESSION=\"$S\" names the DEFAULT session — that is the operator's live herd." ;;
esac

# ── resolve the probe socket ─────────────────────────────────────────────────────────────────────
SOCK="${HERDR_TG_PROBE_SOCKET:-}"
if [ -z "$SOCK" ]; then
  # The only herdr-CLI invocation in this script. Requires the probe session to be RUNNING already.
  # Set HERDR_TG_PROBE_SOCKET to bypass. `.server.socket` is verified present in `herdr status --json`.
  RS="$("$HERDR" --session "$S" status --json 2>"$TMP/resolve.err")" \
    || refuse "\`$HERDR --session $S status --json\` failed — is the probe session running?
  $(head -3 "$TMP/resolve.err")
  (or set HERDR_TG_PROBE_SOCKET=<path> and re-run)"
  SOCK="$(jq -r '.server.socket // empty' <<<"$RS")"
  [ -n "$SOCK" ] || refuse "could not read .server.socket out of \`$HERDR --session $S status --json\`"
fi

[ -S "$SOCK" ] || refuse "resolved probe socket is not a socket: $SOCK"

# Two independent identities, and EITHER matching is a refusal. `stat` does NOT dereference a
# symlink without -L (learned the hard way: a symlink to the live socket sailed through a version
# of this check that compared the un-dereferenced inode), and readlink alone would miss a hard
# link or a bind mount. Belt and braces, because this is the one check that matters.
ident_path(){ readlink -f "$1" 2>/dev/null || printf '%s' "$1"; }
ident_ino(){  stat -L -c '%d:%i' "$1" 2>/dev/null; }
PROBE_PATH="$(ident_path "$SOCK")"
PROBE_INO="$(ident_ino  "$SOCK")"
for LIVE in "$LIVE_DEFAULT" "${HERDR_SOCKET_PATH:-}"; do
  [ -n "$LIVE" ] || continue
  [ -e "$LIVE" ] || continue
  LP="$(ident_path "$LIVE")"; LI="$(ident_ino "$LIVE")"
  if [ "$PROBE_PATH" = "$LP" ] || { [ -n "$PROBE_INO" ] && [ "$PROBE_INO" = "$LI" ]; }; then
    refuse "the resolved socket IS the operator's live herdr socket.
  probe : $SOCK  ->  $PROBE_PATH  [$PROBE_INO]
  live  : $LIVE  ->  $LP  [$LI]
  Sending into that socket types into terminals where real agents are working. This is the one
  thing this script exists to make impossible."
  fi
done

say "probe session : $S"
say "probe socket  : $SOCK"
say "live socket   : $LIVE_DEFAULT (and \$HERDR_SOCKET_PATH=${HERDR_SOCKET_PATH:-<unset>}) — both cleared"

# ── one-shot RPC over the probe socket, and nothing else ────────────────────────────────────────
N=0
rpc(){ # $1 method  $2 params-json
  N=$((N+1))
  local req; req="$(jq -cn --arg m "$1" --arg id "vsp20-$N" --argjson p "$2" '{id:$id,method:$m,params:$p}')"
  printf '>> %s\n' "$req" >> "$AUDIT"
  local rep; rep="$(printf '%s\n' "$req" | socat -t 10 - "UNIX-CONNECT:$SOCK" 2>/dev/null)"
  printf '<< %s\n' "$rep" >> "$AUDIT"
  printf '%s' "$rep"
}
tag(){ jq -r '.result.type // ("ERROR:" + (.error.code // "?") + ":" + (.error.message // ""))' <<<"$1"; }
read_visible(){ rpc pane.read "$(jq -cn --arg p "$1" '{pane_id:$p,source:"visible",format:"text"}')" | jq -j '.result.read.text // ""'; }

# Clear whatever the probe left on the pane's input line.
#
# NOT `C-c`. HERDR_API.md (0.7.0-0.7.4 / protocol 16) is explicit that tmux syntax is rejected —
# `C-c` returns `invalid_key: unsupported key C-c` — and that Ctrl-C is spelled `ctrl+c`. Sending
# `C-c` here would be refused by the validator and the interrupt would silently not happen, leaving
# a half-typed line in the probe pane and the NEXT probe's reading polluted by it. That grammar is
# exactly what P2 below exists to re-settle on p20, so this tries `ctrl+c` first and falls back to
# the tmux spelling if the server refuses it, rather than assuming either is right.
interrupt(){
  local r
  r="$(rpc pane.send_keys "$(jq -cn --arg p "$1" '{pane_id:$p,keys:["ctrl+c"]}')")"
  if [ "$(jq -r '.result.type // empty' <<<"$r")" = "" ]; then
    say "  (ctrl+c refused: $(tag "$r") — retrying the tmux spelling C-c)"
    r="$(rpc pane.send_keys "$(jq -cn --arg p "$1" '{pane_id:$p,keys:["C-c"]}')")"
    [ "$(jq -r '.result.type // empty' <<<"$r")" != "" ] \
      || say "  ⚠ NEITHER ctrl+c NOR C-c was accepted; the probe pane may hold a half-typed line."
  fi
}

SNAP="$(rpc session.snapshot '{}')"
[ "$(tag "$SNAP")" = session_snapshot ] || refuse "probe socket did not answer session.snapshot: $(tag "$SNAP")"

PROTO="$(jq -r '.result.snapshot.protocol' <<<"$SNAP")"
[ "$PROTO" = 20 ] || refuse "probe herd speaks protocol $PROTO, not 20 — this script's findings would not be p20 facts"
NPANES="$(jq '.result.snapshot.panes|length' <<<"$SNAP")"
say "probe herd    : herdr $(jq -r .result.snapshot.version <<<"$SNAP"), protocol $PROTO, $NPANES pane(s)"

[ "$NPANES" -le "$MAXPANES" ] || refuse "the probe session has $NPANES panes (max $MAXPANES).
  A throwaway probe session has one or two panes; a real herd has more. Either you pointed this at
  the wrong session, or raise HERDR_TG_PROBE_MAX_PANES deliberately."

if [ -n "${HERDR_PANE_ID:-}" ] && jq -e --arg me "$HERDR_PANE_ID" '.result.snapshot.panes[]|select(.pane_id==$me)' <<<"$SNAP" >/dev/null; then
  refuse "the probe snapshot contains \$HERDR_PANE_ID ($HERDR_PANE_ID) — that is the pane this shell runs in, so this is not a separate session."
fi

# ── pick and vet the target pane ─────────────────────────────────────────────────────────────────
P="${HERDR_TG_PROBE_PANE:-}"
if [ -z "$P" ]; then
  P="$(jq -r '[.result.snapshot.panes[]|select((.agent // "") == "")][0].pane_id // empty' <<<"$SNAP")"
  [ -n "$P" ] || refuse "no agent-less pane in the probe session. Open a plain shell pane in it (or set HERDR_TG_PROBE_PANE)."
fi
PJ="$(jq -c --arg p "$P" '.result.snapshot.panes[]|select(.pane_id==$p)' <<<"$SNAP")"
[ -n "$PJ" ] || refuse "pane $P is not in the PROBE snapshot. Refusing to send to a pane this script cannot see."
[ "$P" != "${HERDR_PANE_ID:-}" ] || refuse "target pane is \$HERDR_PANE_ID — that is this session's own pane."
AG="$(jq -r '.agent // ""' <<<"$PJ")"
[ -z "$AG" ] || refuse "pane $P has a detected agent (\"$AG\").
  This script sends raw keystrokes; it runs only against a plain shell pane. Open one, or point
  HERDR_TG_PROBE_PANE at it."
say "target pane   : $P  (agent-less, cwd $(jq -r '.cwd // "?"' <<<"$PJ"))"

say ""
say "REFUSAL GATE PASSED — probe socket, probe session, agent-less pane."

# Gate passed: this run is really going to happen, so the transcript earns a place on disk.
mkdir -p "$(dirname "$AUDIT_FINAL")"
mv "$AUDIT" "$AUDIT_FINAL" || refuse "could not place the audit log at $AUDIT_FINAL"
AUDIT="$AUDIT_FINAL"
say "audit log     : $AUDIT"

if [ "$DRY" -eq 1 ]; then
  hdr "DRY RUN — every gate passed and NOTHING was sent."
  say "Re-run without --dry-run to settle P1-P5. Audit log: $AUDIT"
  exit 0
fi

# ══ P1 — the success tag of the three send methods ════════════════════════════════════════════════
# Every payload starts with '#', so anything that reaches the shell is a comment. That is the
# difference between a probe and an accident.
hdr "P1  success tag of pane.send_text / pane.send_keys / pane.send_input"
T1="$(rpc pane.send_text  "$(jq -cn --arg p "$P" '{pane_id:$p,text:"# vsp20 P1 send_text"}')")"
say "pane.send_text   -> $(tag "$T1")   raw: $(jq -c '.result // .error' <<<"$T1")"
T2="$(rpc pane.send_keys  "$(jq -cn --arg p "$P" '{pane_id:$p,keys:["Enter"]}')")"
say "pane.send_keys   -> $(tag "$T2")   raw: $(jq -c '.result // .error' <<<"$T2")"
T3="$(rpc pane.send_input "$(jq -cn --arg p "$P" '{pane_id:$p,text:"# vsp20 P1 send_input",keys:["Enter"]}')")"
say "pane.send_input  -> $(tag "$T3")   raw: $(jq -c '.result // .error' <<<"$T3")"
say ""
say "FINDING P1: the wire tag is whatever is printed above. \`ok\` was INFERRED in the spec from it"
say "  being the only void tag among 58 ResponseResult tags; it is now observed. Note what it does"
say "  NOT mean: herdr took the bytes. Not that the agent received, rendered, parsed or acted on"
say "  them. Slice 3's Telegram confirmation must say \"accepted\", never \"delivered\"."

# ══ P2 — the key grammar ══════════════════════════════════════════════════════════════════════════
hdr "P2  pane.send_keys key grammar on protocol 20"
say "Each key is sent alone, preceded by a fresh '#' so anything that lands is a shell comment."
say "ctrl+d / C-d are DELIBERATELY EXCLUDED in every spelling: EOF would close the pane and end"
say "the probe. So is ctrl+z. The list below is built to CONFIRM OR REFUTE the p16 evidence, so it"
say "carries both the documented '+' chord form and the tmux '-' form the p16 notes say is refused;"
say "a sweep with only one of them cannot tell 'the grammar moved' from 'we asked wrong'."
KEYS=( # p16: special keys, bare, case-insensitive
       "Enter" "enter" "ENTER" "Return" "CR" "Tab" "Escape" "Esc" "Space"
       "Backspace" "BackSpace" "BSpace" "BS" "Up" "Down" "Left" "Right" "F1" "F12"
       # p16: NOT supported in any spelling — expect invalid_key, and a change here is news
       "Home" "End" "PageUp" "PageDown" "Insert" "Delete"
       # p16: chords join with '+', in any modifier order; the tmux '-' forms are refused
       "ctrl+c" "ctrl+u" "ctrl+l" "shift+tab" "alt+f" "alt+Up" "ctrl+shift+p"
       "ctrl+alt+shift+p" "C-c" "ctrl-c" "Ctrl-C" "C-u" "M-b" "BTab"
       # p16: a one-character string is typed as that literal character
       "a" "1" "."
       # malformed on purpose: the client's Key newtype refuses these before the wire, so this is
       # the only place their SERVER-side answer can be recorded at all
       "" " " "\\n" "Enter Enter" )
printf '%-18s %s\n' "key" "result"
for k in "${KEYS[@]}"; do
  rpc pane.send_text "$(jq -cn --arg p "$P" '{pane_id:$p,text:"# "}')" >/dev/null
  R="$(rpc pane.send_keys "$(jq -cn --arg p "$P" --arg k "$k" '{pane_id:$p,keys:[$k]}')")"
  printf '%-18s %s\n' "'$k'" "$(tag "$R")" | tee -a "$AUDIT"
done
interrupt "$P"
say ""
say "Pane after the sweep (visible):"
read_visible "$P" | tail -20 | sed 's/^/  | /' | tee -a "$AUDIT"
say ""
say "FINDING P2: accepted keys are the ones above that did not return invalid_key. The single"
say "  most consequential line is ctrl+c vs C-c: p16 says the '+' form is right and the tmux '-'"
say "  form is invalid_key, and a config file written by hand will use the wrong one. Read the pane"
say "  dump too — herdr accepting a key name is NOT the same as the PTY receiving what you meant."
say "  The per-harness SUBMIT key (claude vs opencode) still has to be settled by hand, in an"
say "  agent pane, in this probe session, because it is a harness fact and not a herdr fact."

# ══ P3 — bracketed paste ══════════════════════════════════════════════════════════════════════════
hdr "P3  does pane.send_input frame its text in bracketed paste?"
say "Sending a two-line payload with NO keys. Both lines are shell comments either way."
read_visible "$P" > "$TMP/before.txt"
PROMPT="$(tail -1 "$TMP/before.txt")"
rpc pane.send_input "$(jq -cn --arg p "$P" '{pane_id:$p,text:"# vsp20 P3 line one\n# vsp20 P3 line two"}')" >/dev/null
sleep 1
read_visible "$P" > "$TMP/after.txt"
say "before (tail):"; tail -6 "$TMP/before.txt" | sed 's/^/  | /' | tee -a "$AUDIT"
say "after (tail):";  tail -8 "$TMP/after.txt"  | sed 's/^/  | /' | tee -a "$AUDIT"
if [ -z "$PROMPT" ]; then
  BP=0; AP=0
  say "(the visible read's last line is empty, so the prompt heuristic below is meaningless — judge P3 by eye)"
else
  BP="$(grep -cF -- "$PROMPT" "$TMP/before.txt" 2>/dev/null)"; BP="${BP:-0}"
  AP="$(grep -cF -- "$PROMPT" "$TMP/after.txt"  2>/dev/null)"; AP="${AP:-0}"
fi
say ""
if [ "$AP" -gt "$BP" ]; then
  say "FINDING P3 (heuristic): the prompt count rose $BP -> $AP, i.e. the embedded \\n EXECUTED."
  say "  => send_input writes RAW bytes; it does NOT bracket-paste."
  say "  CONSEQUENCE, and it is the big one: a multi-line Telegram reply relayed verbatim would run"
  say "  line-by-line in the operator's terminal. Slice 3 MUST escape or re-frame newlines before"
  say "  any send. Confirm by eye against the dump above before you trust this line."
else
  say "FINDING P3 (heuristic): the prompt count did not rise ($BP -> $AP), i.e. both lines sit in the"
  say "  input buffer unexecuted => send_input appears to BRACKET-PASTE (or at least not to execute)."
  say "  Confirm by eye against the dump above; then confirm the submit key separately."
fi
interrupt "$P"

# ══ P4 — does a filtered subscription fire on a later transition? ═════════════════════════════════
hdr "P4  filtered subscription: catch-up-at-subscribe only, or also later transitions?"
say "Driven with pane.report_agent, so no real agent is needed and nothing is typed."
SRC="herdr-tg-verify-send-p20"
rpc pane.report_agent "$(jq -cn --arg p "$P" --arg s "$SRC" '{pane_id:$p,source:$s,agent:"probe",state:"idle"}')" >/dev/null
sleep 1
SUB="$(jq -cn --arg p "$P" '{id:"vsp20-sub",method:"events.subscribe",params:{subscriptions:[{type:"pane.agent_status_changed",pane_id:$p,agent_status:"blocked"}]}}')"
printf '>> %s\n' "$SUB" >> "$AUDIT"
{ printf '%s\n' "$SUB"; sleep 8; } | socat -t 9 - "UNIX-CONNECT:$SOCK" > "$TMP/events.ndjson" 2>/dev/null &
EPID=$!
sleep 2
CATCHUP="$(grep -c 'pane.agent_status_changed' "$TMP/events.ndjson" 2>/dev/null)"; CATCHUP="${CATCHUP:-0}"
say "frames 2 s after subscribing while the pane is idle (filter=blocked): $CATCHUP  [expect 0]"
rpc pane.report_agent "$(jq -cn --arg p "$P" --arg s "$SRC" '{pane_id:$p,source:$s,agent:"probe",state:"blocked"}')" >/dev/null
sleep 3
AFTER="$(grep -c 'pane.agent_status_changed' "$TMP/events.ndjson" 2>/dev/null)"; AFTER="${AFTER:-0}"
wait "$EPID" 2>/dev/null
say "frames after the idle -> blocked transition: $AFTER"
sed 's/^/  << /' "$TMP/events.ndjson" | head -10 | tee -a "$AUDIT"
say ""
if [ "$AFTER" -gt "$CATCHUP" ]; then
  say "FINDING P4: a FILTERED subscription DOES fire on later transitions into the filtered status."
  say "  => slice 3 can pin one filtered subscription per pane and get both recovery-at-subscribe and"
  say "     live edges from the same stream."
else
  say "FINDING P4: a FILTERED subscription is CATCH-UP-AT-SUBSCRIBE ONLY — it did not fire on the"
  say "  transition. => slice 3 needs the UNFILTERED subscription for live edges and a filtered one"
  say "  (or a re-subscribe) for recovery. That is two mechanisms, not one. Re-estimate slice 3."
fi

# ══ P5 — does state_change_seq churn while blocked? ═══════════════════════════════════════════════
hdr "P5  does state_change_seq churn while the pane sits blocked?"
say "The pane is blocked from P4. Sampling every 1 s for 10 s with no further writes."
SEQS=""
for i in $(seq 1 10); do
  Q="$(rpc session.snapshot '{}' | jq -r --arg p "$P" '[.result.snapshot.agents[]|select(.pane_id==$p)][0].state_change_seq // "absent"')"
  SEQS="$SEQS $Q"
  sleep 1
done
say "state_change_seq samples:$SEQS"
U="$(printf '%s\n' $SEQS | sort -u | wc -l)"
say ""
if [ "$U" -le 1 ]; then
  say "FINDING P5: state_change_seq is STABLE while blocked ($U distinct value). => it is a sound"
  say "  dedupe key for slices 3/4, subject to the extra agent.get per event and its race."
else
  say "FINDING P5: state_change_seq CHURNED while blocked ($U distinct values) with no state change."
  say "  => it is NOT a dedupe key. A resubscribe replay after a laptop sleep would re-push asks the"
  say "  operator has already seen. Slice 3 must dedupe on something else (pane_id + status edge)."
fi

# ══ cleanup ═══════════════════════════════════════════════════════════════════════════════════════
hdr "cleanup"
rpc pane.report_agent  "$(jq -cn --arg p "$P" --arg s "$SRC" '{pane_id:$p,source:$s,agent:"probe",state:"idle"}')" >/dev/null
rpc pane.release_agent "$(jq -cn --arg p "$P" --arg s "$SRC" '{pane_id:$p,source:$s,agent:"probe"}')"              >/dev/null
interrupt "$P"
say "reported idle, released the probe agent, interrupted $P."
say ""
say "Pane final state (visible):"
read_visible "$P" | tail -12 | sed 's/^/  | /' | tee -a "$AUDIT"
say ""
say "DONE. $(grep -c '^>> ' "$AUDIT") RPCs, all against $SOCK. Full transcript: $AUDIT"
say "Close the probe session when you are finished with it. Nothing here touched the live herd."
