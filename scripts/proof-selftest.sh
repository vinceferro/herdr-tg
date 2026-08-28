#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────────────────────────
# proof-selftest.sh — proves the proof can FAIL.
#
# A proof harness that only ever prints green is indistinguishable from a harness that is not
# looking. Two independent question sets:
#
#   1. DISCRIMINATION. Eight mutation fakes through `proof-slice1.sh --gates=3`. Required outcome:
#      5 PASS / 3 FAIL. They are driven at gate 3 in isolation because they are shell scripts and
#      would all die at gate 2 by construction — that is the anti-cheat working, not a shortcut.
#
#   2. ANTI-CHEAT. A set of CHEATING clients, each asserted to die at a NAMED gate. The first
#      version of this file had exactly one of these — `fake-cheat.sh`, a shebang and a bare
#      `herdr api snapshot` — and an adversarial reviewer then passed all seven gates with a
#      30-line `#!/bin/sh` that spelled the same command by absolute path. One instance of the
#      weakest constructible cheat is not evidence for a general property, so the set now covers
#      every escape route that has actually been demonstrated:
#
#        bare-name PATH resolution · absolute-path exec · PATH rebuilt inside the sandbox ·
#        the whole-proof bypass verbatim · the herdr CLI itself as the client ·
#        a COMPILED ELF that shells out (the one the ELF magic check cannot catch) ·
#        canned strings for gate 5 · canned strings for gate 6 · gate 3's empty-vs-empty vacuity
#
# Read-only against the live herd: every fake and every cheat issues `session.snapshot` /
# `pane.read --source visible` and nothing else. No pane.send_*, no writes of any kind.
#
#   usage: ./scripts/proof-selftest.sh [--attempts=N]
# ─────────────────────────────────────────────────────────────────────────────────────────────────
set -uo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
PROOF="$DIR/proof-slice1.sh"
FAKES="$DIR/fakes"
HERDR="${HERDR:-herdr}"
ATTEMPTS="${ATTEMPTS:-5}"
FLAPSTATE="$HOME/.cache/herdr-tg-proof"
VACUITY="$HOME/.cache/herdr-tg-proof-vacuity.marker"

for arg in "$@"; do
  case "$arg" in
    --attempts=*) ATTEMPTS="${arg#--attempts=}" ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$arg" >&2; exit 2 ;;
  esac
done

LOGDIR="$(mktemp -d)"; trap 'rm -rf "$LOGDIR"; rm -f "$VACUITY"' EXIT

fail_hard(){ printf '\nSELFTEST ABORTED: %s\n' "$1" >&2; exit 2; }

# ── preconditions ────────────────────────────────────────────────────────────────────────────────
# Abort loudly rather than reporting a false split: every fake reads the live socket, and a dead
# herd would make all eight "FAIL" and the 5/3 assertion would look like a broken proof.
[ -x "$PROOF" ]  || fail_hard "missing $PROOF"
[ -d "$FAKES" ]  || fail_hard "missing $FAKES"
command -v jq    >/dev/null || fail_hard "jq is not on PATH"
command -v socat >/dev/null || fail_hard "socat is not on PATH (the fakes dial the socket with it)"
"$HERDR" api snapshot >/dev/null 2>&1 || fail_hard "\`$HERDR api snapshot\` failed — the herd must be up; the fakes read the live socket"

# The scratch set shipped fake-wireorder.sh byte-identical to fake-honest.sh below the shebang, so
# the wire-key-order case had never been exercised (SLICE-1.md "Fix before shipping"). Assert it.
if diff -q <(tail -n +2 "$FAKES/fake-honest.sh") <(tail -n +2 "$FAKES/fake-wireorder.sh") >/dev/null 2>&1; then
  fail_hard "fake-wireorder.sh is byte-identical to fake-honest.sh below the shebang — the wire-order case is not being exercised"
fi
# And that they really differ ON THE WIRE, not just in source.
rm -rf "$FLAPSTATE"
E="$(mktemp -d)"
env -i HOME="$HOME" PATH="$E" "$FAKES/fake-honest.sh"    status --json > "$LOGDIR/wire-honest.json" 2>/dev/null
env -i HOME="$HOME" PATH="$E" "$FAKES/fake-wireorder.sh" status --json > "$LOGDIR/wire-order.json"  2>/dev/null
cmp -s "$LOGDIR/wire-honest.json" "$LOGDIR/wire-order.json" \
  && fail_hard "fake-wireorder.sh emits the same bytes as fake-honest.sh — it is not reordering anything"
rmdir "$E" 2>/dev/null

# fake-flap.sh must pass VIA THE SANDWICH, not because its mutation is normalized away. Assert the
# stale first call is genuinely detectable: normalized, it must differ from the reference. Without
# this, a normalize.jq that dropped focused_workspace_id would make the flap case pass vacuously
# and gate 3's client -> reference -> client shape would be untested.
rm -rf "$FLAPSTATE"
E="$(mktemp -d)"
env -i HOME="$HOME" PATH="$E" "$FAKES/fake-flap.sh" status --json 2>/dev/null \
  | jq -S -f "$DIR/normalize.jq" > "$LOGDIR/flap-first.norm"
"$HERDR" api snapshot 2>/dev/null | jq -S -f "$DIR/normalize.jq" > "$LOGDIR/ref.norm"
rm -rf "$FLAPSTATE"; rmdir "$E" 2>/dev/null
[ -s "$LOGDIR/flap-first.norm" ] || fail_hard "fake-flap.sh produced no parseable JSON on its first call"
if cmp -s "$LOGDIR/flap-first.norm" "$LOGDIR/ref.norm"; then
  fail_hard "fake-flap.sh's stale first call canonicalizes IDENTICAL to the reference — its mutation is being normalized away, so 'PASS via the sandwich' would be vacuous"
fi

# ── 1. the mutation set: does gate 3 discriminate? ───────────────────────────────────────────────
# name : required verdict : what it mutates
CASES=(
  "fake-honest.sh|PASS|unmutated control (socket RPC -> jq -c .)"
  "fake-wireorder.sh|PASS|every object's keys re-emitted in reverse order"
  "fake-nullpad.sh|PASS|explicit \"label\":null where herdr omits the key"
  "fake-extracounter.sh|PASS|revision mutated (+1000) on panes and agents"
  "fake-flap.sh|PASS|stale focused_workspace_id on the FIRST call only (sandwich)"
  "fake-dropped_ws.sh|FAIL|drops a workspace"
  "fake-stale_status.sh|FAIL|flips one pane's agent_status"
  "fake-truncated_cwd.sh|FAIL|truncates every pane cwd"
)

printf 'proof-selftest — %s mutation fakes through `proof-slice1.sh --gates=3` (ATTEMPTS=%s)\n\n' "${#CASES[@]}" "$ATTEMPTS"
printf '%-24s %-6s %-6s %s\n' "fake" "want" "got" "mutation"
printf '%s\n' "----------------------------------------------------------------------------------------"

NPASS=0; NFAIL=0; BAD=0
for c in "${CASES[@]}"; do
  IFS='|' read -r NAME WANT WHAT <<<"$c"
  BINPATH="$FAKES/$NAME"
  [ -x "$BINPATH" ] || fail_hard "missing or non-executable fake: $BINPATH"
  rm -rf "$FLAPSTATE"                       # fake-flap.sh counts calls across processes
  BIN="$BINPATH" HERDR="$HERDR" ATTEMPTS="$ATTEMPTS" \
    bash "$PROOF" --gates=3 >"$LOGDIR/$NAME.out" 2>"$LOGDIR/$NAME.err"
  RC=$?
  if [ "$RC" -eq 0 ]; then GOT=PASS; NPASS=$((NPASS+1)); else GOT=FAIL; NFAIL=$((NFAIL+1)); fi
  MARK=" "; if [ "$GOT" != "$WANT" ]; then MARK="<-- WRONG"; BAD=$((BAD+1)); fi
  printf '%-24s %-6s %-6s %s %s\n' "$NAME" "$WANT" "$GOT" "$WHAT" "$MARK"
  if [ "$GOT" != "$WANT" ]; then
    { echo "    ---- $NAME stdout ----"; sed 's/^/    /' "$LOGDIR/$NAME.out"
      echo "    ---- $NAME stderr (first 25 lines) ----"; head -25 "$LOGDIR/$NAME.err" | sed 's/^/    /'; } >&2
  fi
done
rm -rf "$FLAPSTATE"

printf '\n%s PASS / %s FAIL   (required: 5 PASS / 3 FAIL)\n' "$NPASS" "$NFAIL"
[ "$NPASS" -eq 5 ] && [ "$NFAIL" -eq 3 ] || BAD=$((BAD+1))

# ── 2. the cheat set: does the proof refuse a client that is not one? ────────────────────────────
# label | $BIN | extra env | gate selector | the gate it MUST die at
HERDR_ABS="$(command -v "$HERDR" 2>/dev/null || printf '%s' "$HERDR")"
CHEATS=(
  "bare-name herdr|$FAKES/fake-cheat.sh||--gates=0,1,2,3,4,5,6|2"
  "absolute-path herdr|$FAKES/fake-cheat-abs.sh||--gates=0,1,2,3,4,5,6|2"
  "PATH rebuilt in-sandbox|$FAKES/fake-cheat-path.sh||--gates=0,1,2,3,4,5,6|2"
  "whole-proof bypass|$FAKES/fake-cheat-full.sh||--gates=0,1,2,3,4,5,6|2"
  "the herdr CLI itself|$HERDR_ABS||--gates=0,1,2,3,4,5,6|2"
  "canned gate-5 strings|$FAKES/fake-cheat-canned.sh||--gates=0,5|5a"
  "canned gate-6 strings|$FAKES/fake-cheat-canned.sh||--gates=6|6b"
  "forged gate-6 journal|$FAKES/fake-cheat-forge.sh||--gates=6|6b"
  "gate-3 empty-vs-empty|$FAKES/fake-cheat-empty.sh|HERDR=/bin/false|--gates=3|3"
)

# The one cheat that needs a compiler: a real ELF that shells out. The ELF magic check cannot catch
# it — only the namespace neutering can — so without this case the selftest would be asserting one
# sandbox layer and assuming the other. Verified on this box: it passes the PRE-FIX proof 7/7.
ELFSRC="$FAKES/elf-cheat-shellout.c"
ELFBIN=""
CC="$(command -v cc || command -v gcc || true)"
if [ -n "$CC" ] && [ -f "$ELFSRC" ]; then
  ELFBIN="$LOGDIR/elf-cheat-shellout"
  if "$CC" -O0 -o "$ELFBIN" "$ELFSRC" 2>"$LOGDIR/cc.err"; then
    CHEATS+=("compiled ELF, shells out|$ELFBIN||--gates=0,1,2,3,4,5,6|2")
  else
    ELFBIN=""
  fi
fi

printf '\n--- the cheat set: every one must die, at the named gate ---\n'
printf '%-26s %-8s %-10s %s\n' "cheat" "want" "got" "why it died"
printf '%s\n' "----------------------------------------------------------------------------------------"

RULED_OUT=0; CHEAT_BAD=0
for c in "${CHEATS[@]}"; do
  IFS='|' read -r LABEL CBIN CENV CGATES CWANT <<<"$c"
  if [ ! -x "$CBIN" ]; then
    printf '%-26s %-8s %-10s %s\n' "$LABEL" "FAIL@$CWANT" "MISSING" "$CBIN is not executable"
    CHEAT_BAD=$((CHEAT_BAD+1)); continue
  fi
  rm -f "$VACUITY"; rm -rf "$FLAPSTATE"
  SAFE="$(printf '%s' "$LABEL" | tr -c 'A-Za-z0-9' '_')"
  env BIN="$CBIN" HERDR="$HERDR" ${CENV:+"$CENV"} \
    bash "$PROOF" "$CGATES" >"$LOGDIR/cheat-$SAFE.out" 2>"$LOGDIR/cheat-$SAFE.err"
  CRC=$?
  rm -f "$VACUITY"
  GOTGATE="$(sed -n 's/^gate \([0-9][a-z]*\) .*FAIL.*/\1/p' "$LOGDIR/cheat-$SAFE.err" | head -1)"
  WHY="$(sed -n 's/^gate [0-9][a-z]* .*FAIL  *//p' "$LOGDIR/cheat-$SAFE.err" | head -1 | cut -c1-46)"
  if [ "$CRC" -eq 0 ]; then
    printf '%-26s %-8s %-10s %s\n' "$LABEL" "FAIL@$CWANT" "PASSED" "<-- THE PROOF IS THEATRE"
    CHEAT_BAD=$((CHEAT_BAD+1))
  elif [ "$GOTGATE" = "$CWANT" ]; then
    printf '%-26s %-8s %-10s %s\n' "$LABEL" "FAIL@$CWANT" "FAIL@$GOTGATE" "$WHY"
    RULED_OUT=$((RULED_OUT+1))
  else
    printf '%-26s %-8s %-10s %s\n' "$LABEL" "FAIL@$CWANT" "FAIL@${GOTGATE:-?}" "<-- caught, but by the WRONG gate: $WHY"
    CHEAT_BAD=$((CHEAT_BAD+1))
    sed 's/^/    /' "$LOGDIR/cheat-$SAFE.err" | head -12 >&2
  fi
done
BAD=$((BAD + CHEAT_BAD))

SKIPPED=""
if [ -z "$ELFBIN" ]; then
  SKIPPED=" — WARNING: the compiled-ELF shell-out cheat was SKIPPED (no C compiler), so the namespace layer is asserted only by gate 2's own self-check and by the herdr-CLI-as-client case"
fi

# ── verdict ──────────────────────────────────────────────────────────────────────────────────────
echo
if [ "$BAD" -eq 0 ]; then
  printf 'PROOF SELFTEST: PASS — gate 3 discriminates (5 PASS / 3 FAIL) and %s cheating clients were ruled out, each at the gate named above%s\n' \
    "$RULED_OUT" "$SKIPPED"
  exit 0
fi
if [ "$CHEAT_BAD" -gt 0 ]; then
  cat >&2 <<'EOF'

BLOCKER — a cheating client got further than it should have.
A script or a wrapper around the `herdr` CLI is not a herdr-tg client, and a client that prints a
canned string has not decoded anything. If one of these passed, the sandbox or the witness is not
being applied and every green the proof has ever printed is theatre. Do not report slice 1 as
proven. Fix the named gate before anything else.
EOF
fi
echo "PROOF SELFTEST: FAIL — $BAD assertion(s) wrong (see stderr)" >&2
exit 1
