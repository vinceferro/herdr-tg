#!/usr/bin/env bash
# The watchdog's behaviour, pinned. Sends nothing: every case runs with HERDR_TG_ENV_FILE=/dev/null,
# so the send always fails and what is under test is the DECISION.
#
#     bash scripts/watchdog-selftest.sh
#
# Each case is a past finding or a stated requirement. The two that matter most are the first and
# the last pair: a hub that has never run must be perfectly silent, and a hub whose stamp was
# DELETED must not be mistaken for one. Reading those two as the same state is how an alarm turns
# itself off in a way nobody can see.

set -uo pipefail
W="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/deploy/herdr-tg-watchdog.sh"
E=/dev/null
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
pass=0; fail=0
ago() { printf '%s 0\n' "$(( $(date +%s) - ${1:-60} ))"; }
ok()   { printf '  ok   %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  FAIL %s\n' "$1"; fail=$((fail+1)); }
rc_is() { [ "$2" = "$3" ] && ok "$1 (rc=$3)" || bad "$1: want rc=$2, got rc=$3"; }

case_dir() { local d="$T/$1"; mkdir -p "$d"; printf '%s' "$d"; }
wd() { HERDR_TG_STATE_DIR="$1" HERDR_TG_ENV_FILE="$E" bash "$W" "${@:2}"; }

printf 'watchdog selftest\n\n'

d=$(case_dir never); out=$(wd "$d" 2>&1); rc=$?
rc_is "a hub that never ran is silent" 0 $rc
[ -z "$out" ] && ok "and says nothing at all" || bad "it spoke: $out"

d=$(case_dir fresh); : > "$d/hub.heartbeat"; ago > "$d/watchdog.tick"
out=$(wd "$d" 2>&1); rc=$?
rc_is "a stamping hub is silent" 0 $rc
[ -e "$d/watchdog.armed" ] && ok "seeing one stamp arms it for good" || bad "it did not arm"

d=$(case_dir stale); : > "$d/hub.heartbeat"; touch -d '1 hour ago' "$d/hub.heartbeat"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" 2>&1); rc=$?
rc_is "a stale stamp alarms, and a failed send exits non-zero" 1 $rc
printf '%s' "$out" | grep -q 'operator was NOT told' && ok "it says the operator was not told" || bad "silent about the failure: $out"
[ "$(cat "$d/watchdog.latch" 2>/dev/null)" = "1" ] && ok "the latch is written BEFORE the send" || bad "no latch"

d=$(case_dir deleted); : > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1); rc=$?
rc_is "a DELETED stamp alarms rather than disarming" 0 $rc
printf '%s' "$out" | grep -q 'stamp has been deleted' && ok "and says which of the two it is" || bad "wrong text: $out"

d=$(case_dir args)
wd "$d" --help    >/dev/null 2>&1; rc_is "--help sends nothing"        0 $?
wd "$d" --bogus   >/dev/null 2>&1; rc_is "an unknown argument refuses" 2 $?
wd "$d" a b       >/dev/null 2>&1; rc_is "two arguments refuse"        2 $?

d=$(case_dir disarm); : > "$d/hub.heartbeat"; touch -d '1 hour ago' "$d/hub.heartbeat"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"; : > "$d/watchdog.disarmed"
wd "$d" >/dev/null 2>&1; rc_is "a fresh disarm silences it" 0 $?
touch -d '2 days ago' "$d/watchdog.disarmed"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'disarm expired' && ok "an old disarm wears off on its own" || bad "the silence was permanent: $out"

d=$(case_dir badcfg)
HERDR_TG_STATE_DIR="$d" HERDR_TG_ENV_FILE=$E HERDR_TG_STALE_AFTER=3min bash "$W" >/dev/null 2>&1
rc_is "a systemd-shaped '3min' is refused, not read as 0" 78 $?

d=$(case_dir suspend); : > "$d/hub.heartbeat"; touch -d '4 hours ago' "$d/hub.heartbeat"
: > "$d/watchdog.armed"; ago 14400 > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1); rc=$?
rc_is "a four-hour suspend does not fire a false alarm" 0 $rc
[ -z "$out" ] && ok "and the hub gets a full window to stamp" || bad "it spoke: $out"

d=$(case_dir skew); : > "$d/hub.heartbeat"; touch -d '+1 hour' "$d/hub.heartbeat"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'future' && ok "a stamp from the future is not read as fresh" || bad "skew blinded it: $out"

printf '\npass=%s fail=%s\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
