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
# A temp directory that is not an absolute path is not a temp directory. An agent session inherits
# TMPDIR as the literal string `%h/.cache/tmp`, which mktemp reads as RELATIVE and creates inside
# whatever directory this was run from — nine hundred megabytes of state-directory fixtures piled up
# under a folder called `%h` in the repo before anyone noticed, because it is gitignored and silent.
case "${TMPDIR:-/tmp}" in /*) ;; *) TMPDIR=/tmp ;; esac
export TMPDIR
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
pass=0; fail=0
ago() { printf '%s 0\n' "$(( $(date +%s) - ${1:-60} ))"; }
ok()   { printf '  ok   %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  FAIL %s\n' "$1"; fail=$((fail+1)); }
rc_is() { [ "$2" = "$3" ] && ok "$1 (rc=$3)" || bad "$1: want rc=$2, got rc=$3"; }

case_dir() { local d="$T/$1"; mkdir -p "$d"; printf '%s' "$d"; }
wd() { HERDR_TG_STATE_DIR="$1" HERDR_TG_ENV_FILE="$E" bash "$W" "${@:2}"; }

# stamp <dir> <seconds ago> — the heartbeat, aged on the wall clock, which is the clock the
# watchdog reads. The hub writes a word into it; staging it empty would test a shape only a fixture
# can produce.
stamp() { printf 'serving\n' > "$1/hub.heartbeat"; touch -d "$2 seconds ago" "$1/hub.heartbeat"; }
# note <dir> <seconds ago> <word> <phone sentence> <door sentence> [update line sentence] —
# hub.health exactly as heartbeat.rs writes it: the word, then one sentence per leg, rewritten every
# tick. The update line's sentence is optional here on purpose: a hub built before that leg existed
# writes three lines, and a three-line note has to go on being read exactly the way it always was.
note() {
  { printf '%s\n%s\n%s\n' "$3" "$4" "$5"
    if [ "$#" -ge 6 ]; then printf '%s\n' "$6"; fi
  } > "$1/hub.health"
  touch -d "$2 seconds ago" "$1/hub.health"
}
PHONE_OK="the phone line answered 12 seconds ago"
PHONE_BAD="the phone line last answered 5 minutes ago"
DOOR_OK="the agents' door let a connection through 12 seconds ago"
DOOR_BAD="the agents' door has let nothing through since this hub started"
UPD_OK="the hub looked for your taps 12 seconds ago"
UPD_BAD="the hub last looked for your taps 5 minutes ago"
UPD_HELD="another copy of this bot is taking your taps, so none of them reach the agents here"

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

# ── the hub is two halves, and they fail apart ───────────────────────────────
# get_me answering proves the bot can talk to Telegram. It proves nothing about the door agents
# arrive at, and a hub whose socket never opened answers Telegram every forty-five seconds forever.
# The hub now withholds its stamp when EITHER half fails, so staleness alone raises the alarm — but
# staleness cannot say WHICH half, and the operator's next move differs: one is "look at the
# machine", the other is "restart the hub". hub.health is where the hub says which, every tick.

d=$(case_dir bothfresh); stamp "$d" 10; note "$d" 10 serving "$PHONE_OK" "$DOOR_OK"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1); rc=$?
rc_is "a hub proving both halves stays quiet" 0 $rc
[ -z "$out" ] && ok "and it says nothing at all" || bad "it spoke: $out"

d=$(case_dir doordown); stamp "$d" 3600; note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1); rc=$?
rc_is "a stamp withheld because the agents' door died alarms" 0 $rc
printf '%s' "$out" | grep -q 'agents cannot reach it' \
  && ok "and the alarm names the agents' side, because that is the half that is down" \
  || bad "the door went quiet and the alarm did not say so: $out"
printf '%s' "$out" | grep -q 'phone line is down' \
  && bad "it blamed the phone line, which the hub says is answering: $out" \
  || ok "and it does not blame the phone line, which the hub says is answering"
printf '%s' "$out" | grep -qF "The hub says: $DOOR_BAD" \
  && ok "and it repeats what the hub said about that half, so he knows what to fix" \
  || bad "the hub's own reason was dropped: $out"

d=$(case_dir phonedown); stamp "$d" 3600; note "$d" 10 'not serving' "$PHONE_BAD" "$DOOR_OK"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1); rc=$?
rc_is "a stamp withheld because the phone line stopped answering alarms" 0 $rc
printf '%s' "$out" | grep -q 'phone line is down' \
  && ok "and the alarm names the phone line" || bad "wrong half named: $out"
printf '%s' "$out" | grep -q 'agents cannot reach it' \
  && bad "it blamed the agents' door, which the hub says is fine: $out" \
  || ok "and it does not blame the agents' door, which the hub says is fine"

d=$(case_dir bothdown); stamp "$d" 3600; note "$d" 10 'not serving' "$PHONE_BAD" "$DOOR_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'and agents cannot reach it either' \
  && ok "a hub that lost both halves is described as both, not as one of them" \
  || bad "it named only one half of a total outage: $out"

# The note is rewritten on EVERY tick, green or not. A note as old as the stamp is therefore not a
# hub explaining what it is withholding — it is the last thing a hub that has since died managed to
# say, and reading it would name a half that stopped long before the process did.
d=$(case_dir hubdead); stamp "$d" 3600; note "$d" 3600 'not serving' "$PHONE_OK" "$DOOR_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'gone quiet' \
  && ok "a hub that died outright is not diagnosed from the note it left behind" \
  || bad "it read a dead hub's last words as a live report: $out"
printf '%s' "$out" | grep -q 'agents cannot reach it' \
  && bad "it named the half that stopped first as though it were why the process is gone: $out" \
  || ok "and it names no half, because a stale note names the wrong one"
printf '%s' "$out" | grep -q 'The hub says' \
  && bad "it quoted a note as stale as the stamp: $out" \
  || ok "and it quotes nothing, because nothing there is current"

d=$(case_dir nonote); stamp "$d" 3600; : > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1); rc=$?
rc_is "a hub built before hub.health existed still alarms" 0 $rc
printf '%s' "$out" | grep -q 'gone quiet' \
  && ok "and is told the herd went quiet, which is all that is known without a note" \
  || bad "a hub with no note was given a diagnosis out of nowhere: $out"

d=$(case_dir tornnote); stamp "$d" 3600; printf 'not serving\n' > "$d/hub.health"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'gone quiet' \
  && ok "a note caught half-written is not turned into a diagnosis" \
  || bad "an unreadable note changed the alarm: $out"
printf '%s' "$out" | grep -q 'The hub says' \
  && bad "it quoted a half-written note: $out" || ok "and nothing is quoted from it"

# The stamp is the fact. The note is only the hub's account of itself, and a hub sick enough to be
# withholding its stamp is exactly the hub whose account cannot be trusted to overrule it.
d=$(case_dir liar); stamp "$d" 3600; note "$d" 10 serving "$PHONE_OK" "$DOOR_OK"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'would send' \
  && ok "a note claiming all is well does not talk the watchdog out of a stopped stamp" \
  || bad "the hub argued its way out of its own alarm: $out"

# One counter for "an outage is in progress" would let the door's half-hourly repeat swallow the
# FIRST alarm of the phone line going down twenty minutes later — the one message with no second
# chance. Which halves are down is remembered, and a change to that set is news.
d=$(case_dir changed); stamp "$d" 3600; note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
wd "$d" >/dev/null 2>&1
[ "$(cat "$d/watchdog.latch" 2>/dev/null)" = "1" ] && ok "the first alarm latches" || bad "no latch"
note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_BAD"; ago > "$d/watchdog.tick"
wd "$d" >/dev/null 2>&1
[ "$(cat "$d/watchdog.latch" 2>/dev/null)" = "2" ] && ok "and the same outage a minute later is held back" \
                                                   || bad "it repeated itself: $(cat "$d/watchdog.latch")"
note "$d" 10 'not serving' "$PHONE_BAD" "$DOOR_BAD"; ago > "$d/watchdog.tick"
wd "$d" >/dev/null 2>&1
[ "$(cat "$d/watchdog.latch" 2>/dev/null)" = "1" ] \
  && ok "but the other half going down too is told at once, not in half an hour" \
  || bad "a second, different outage was swallowed by the first one's repeat interval"

# One continuous outage whose OTHER half flaps — a laptop roaming wifi while the door stays shut —
# changed the failing set on every other check, and any change of set was treated as news. That
# turned the repeat interval off completely: one message a minute, all night, which is the exact
# failure the latch is the only thing standing between. A half already named in this outage is not
# news again.
d=$(case_dir flapping); : > "$d/watchdog.armed"; sent=0
for i in 1 2 3 4 5 6 7 8 9 10; do
  ago > "$d/watchdog.tick"; stamp "$d" $(( 200 + i * 60 ))
  if [ $(( i % 2 )) -eq 0 ]; then note "$d" 30 'not serving' "$PHONE_BAD" "$DOOR_BAD"
  else                            note "$d" 30 'not serving' "$PHONE_OK"  "$DOOR_BAD"; fi
  out=$(wd "$d" 2>&1)
  printf '%s' "$out" | grep -q 'operator was NOT told' && sent=$(( sent + 1 ))
done
[ "$sent" -le 2 ] \
  && ok "a half that keeps flapping does not become a message a minute ($sent alarms in 10 checks)" \
  || bad "one continuous outage sent $sent alarms in 10 checks; the repeat interval never applied"

# The note is rewritten every forty-five seconds by a hub that is running at all. One older than two
# of those ticks is not a live hub explaining what it is withholding — it is the last thing a hub
# that has since died managed to say, and it names the half that stopped FIRST rather than the fact
# that the whole process is now gone. Reading it tells him his phone still works when there is no
# bot at all. The window used to be the staleness window itself, which is by definition wide enough
# to admit every such note.
d=$(case_dir hubgone); stamp "$d" 205; note "$d" 150 'not serving' "$PHONE_OK" "$DOOR_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'phone line is up' \
  && bad "a hub that has written nothing for two and a half minutes is still described as answering: $out" \
  || ok "a note older than two hub ticks is not read as a live hub's report"
printf '%s' "$out" | grep -q 'The hub says' \
  && bad "it quoted a dead hub's last words: $out" \
  || ok "and it quotes nothing, because nothing there is current"

# A sentence this script does not recognise is a hub built after it — the script is copied into
# place at install time, so a repo whose hub has moved on while the installed watchdog has not is an
# ordinary state, not a contrived one. Reading an unknown sentence as "this half is unwell" made
# every outage read as total AND quoted, as the evidence of the fault, a line saying the half was
# fine. A note it cannot read leaves it saying what it has always said.
d=$(case_dir reworded); stamp "$d" 3600
note "$d" 10 'not serving' "Telegram answered 12 seconds ago" "$DOOR_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'and agents cannot reach it either' \
  && bad "a sentence it could not read was reported as that half being down: $out" \
  || ok "a sentence this watchdog does not know is not turned into a diagnosis"
printf '%s' "$out" | grep -q 'The hub says' \
  && bad "it quoted a note it could not read: $out" \
  || ok "and it quotes nothing out of a note it cannot read"

# The headline case of the whole change, and it was silent. Arming on the stamp alone means a hub
# whose door NEVER opened — no forum configured, a socket that would not bind, a wiped state dir —
# never earns a first stamp, so the watchdog never arms and says nothing for the life of the box,
# while the bot answers Telegram every forty-five seconds and no agent can reach anybody. The hub
# writes its note on every tick whichever way the verdict went, so the note is the proof a hub has
# run here.
d=$(case_dir neverstamped); note "$d" 10 'not serving' "$PHONE_OK" "the agents' door could not be opened"
ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1); rc=$?
rc_is "a hub that has never earned a stamp still arms the alarm" 0 $rc
printf '%s' "$out" | grep -q 'would send' \
  && ok "and a door that never opened is an alarm, not silence" \
  || bad "the box item 4 exists for got no alarm at all: $out"
[ -e "$d/watchdog.armed" ] && ok "and seeing a hub's own note arms it for good" || bad "it did not arm"
printf '%s' "$out" | grep -qF "The hub says: the agents' door could not be opened" \
  && ok "and it repeats the hub's reason, which is the one thing he can act on" \
  || bad "the hub's reason was dropped: $out"

# The stamp gone while the note is still being written every tick. That is not proof the hub never
# worked — it is a stamp that was removed, from a state dir somebody tidied or a path that moved —
# and the one message that has to be trusted must not tell him this hub has never once served when
# it may well have served all week.
d=$(case_dir stampgone); note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'never once' \
  && bad "a stamp that was removed was reported as a hub that never worked: $out" \
  || ok "a missing stamp beside a live note is not called a hub that never served"
printf '%s' "$out" | grep -q 'would send' \
  && ok "and it is still an alarm, because nothing is stamping now" \
  || bad "it went quiet on a hub with no stamp at all: $out"

# What the hub knows is that get_me answered. That is not the same as "your phone still works": a
# second copy of the bot holding the long poll leaves get_me answering while nothing he taps moves.
# The one message that has to be trusted must not claim more than the fact behind it.
d=$(case_dir doorclaim); stamp "$d" 3600; note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'Your phone still works' \
  && bad "the alarm promised more about the phone line than the hub can know: $out" \
  || ok "the door alarm says what answered, not that his phone works"
printf '%s' "$out" | grep -q 'The bot is still answering Telegram' \
  && ok "and it says which fact it has, so he knows what was actually proved" \
  || bad "it does not say what the hub actually proved: $out"

# ── the third half: the line Telegram sends his taps down ───────────────────
# get_me answering proves the bot can REACH Telegram. It proves nothing about Telegram sending
# anything back, and those fail apart: a second copy of the bot holding the long poll leaves get_me
# answering every forty-five seconds while every tap the operator makes is handed to the other copy
# and dies there. It is the quietest outage this system has — his phone looks perfect, the agents
# look busy, and nothing he does arrives. So the hub withholds its stamp for this half too, and says
# so on a fourth line of the note.

d=$(case_dir updatesdown); stamp "$d" 3600
note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_OK" "$UPD_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1); rc=$?
rc_is "a stamp withheld because Telegram stopped sending updates alarms" 0 $rc
printf '%s' "$out" | grep -q 'nothing you tap is reaching an agent' \
  && ok "and the alarm names the update line, because that is the half that is down" \
  || bad "the taps were dying in silence and the alarm did not say so: $out"
printf '%s' "$out" | grep -q 'phone line is down' \
  && bad "it blamed the phone line, which the hub says is answering: $out" \
  || ok "and it does not blame the phone line, which the hub says is answering"
printf '%s' "$out" | grep -q 'agents cannot reach it' \
  && bad "it blamed the agents' door, which the hub says is letting connections through: $out" \
  || ok "and it does not blame the agents' door, which the hub says is fine"
printf '%s' "$out" | grep -qF "The hub says: $UPD_BAD" \
  && ok "and it repeats what the hub said about that half" \
  || bad "the hub's own reason was dropped: $out"

# The one cause worth naming. A second copy of the bot — a stray `serve` in a terminal, a unit
# started twice, a laptop and a server sharing one token — is far and away the likeliest reason the
# update line goes quiet, and it is the one thing he can check in ten seconds. An alarm that says
# "updates are not flowing" and stops there sends him to read the journal instead.
d=$(case_dir secondcopy); stamp "$d" 3600
note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_OK" "$UPD_HELD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -qF "The hub says: $UPD_HELD" \
  && ok "a second copy of the bot holding the update line is quoted to him in those words" \
  || bad "the one reason he can act on was dropped: $out"
printf '%s' "$out" | grep -q 'second copy of this bot' \
  && ok "and the alarm itself names the likeliest cause, so he knows what to look for" \
  || bad "it named the half but not the thing to go and check: $out"

d=$(case_dir allthree); stamp "$d" 3600
note "$d" 10 'not serving' "$PHONE_BAD" "$DOOR_BAD" "$UPD_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'phone line is down' \
  && ok "a hub that lost all three halves has the phone line named" \
  || bad "a total outage did not name the phone line: $out"
printf '%s' "$out" | grep -q 'agents cannot reach it' \
  && ok "and the agents' door is named too" || bad "a total outage did not name the door: $out"
printf '%s' "$out" | grep -q 'nothing you tap is reaching an agent' \
  && ok "and so is the update line" || bad "a total outage did not name the update line: $out"

d=$(case_dir doorandupdates); stamp "$d" 3600
note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_BAD" "$UPD_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'nothing you tap is reaching an agent' \
  && ok "two halves down together are both named, not just the first one found" \
  || bad "the update line was swallowed by the door's outage: $out"
printf '%s' "$out" | grep -q 'agents cannot reach it' \
  && ok "and so is the other" || bad "the door was swallowed by the update line's outage: $out"
printf '%s' "$out" | grep -q 'phone line is down' \
  && bad "it blamed a phone line the hub says is answering: $out" \
  || ok "and the half that is working is not blamed"

d=$(case_dir phoneandupdates); stamp "$d" 3600
note "$d" 10 'not serving' "$PHONE_BAD" "$DOOR_OK" "$UPD_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'phone line is down' \
  && ok "a dead phone line beside a dead update line names the phone line" \
  || bad "the phone line was not named: $out"
printf '%s' "$out" | grep -q 'agents cannot reach it' \
  && bad "it blamed a door the hub says is letting connections through: $out" \
  || ok "and does not blame the door, which the hub says is fine"

d=$(case_dir threefresh); stamp "$d" 10
note "$d" 10 serving "$PHONE_OK" "$DOOR_OK" "$UPD_OK"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1); rc=$?
rc_is "a hub proving all three halves stays quiet" 0 $rc
[ -z "$out" ] && ok "and it says nothing at all" || bad "it spoke: $out"

# A hub from before the update line was a leg writes three lines. That is not a fault on the fourth
# half — it is a hub that cannot say anything about it — and the two halves it DOES describe must go
# on being named exactly as they were, or upgrading the hub is what stops the alarm working.
d=$(case_dir threelinenote); stamp "$d" 3600
note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_BAD"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'agents cannot reach it' \
  && ok "a note that says nothing about the update line still names the halves it does describe" \
  || bad "a three-line note stopped naming the door it describes: $out"
printf '%s' "$out" | grep -q 'nothing you tap is reaching an agent' \
  && bad "a half the hub said nothing about was reported as down: $out" \
  || ok "and a half it says nothing about is not turned into a fault"

# Same rule as the other two legs: an unknown sentence is a hub newer than this copy of the script,
# not a broken half. Reading it as broken would report every door outage as a tap outage as well.
d=$(case_dir updreworded); stamp "$d" 3600
note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_BAD" "the long poll returned 409 Conflict"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'nothing you tap is reaching an agent' \
  && bad "a sentence it could not read was reported as that half being down: $out" \
  || ok "an update line sentence this watchdog does not know is not turned into a diagnosis"
printf '%s' "$out" | grep -q '409' \
  && bad "it quoted a sentence it could not classify: $out" \
  || ok "and it quotes nothing out of a line it cannot read"

# The update leg's GOOD sentence, on its own, with nothing else to decide the shape. Every other
# update-leg case here holds a broken door or a broken phone line as well, so the shape is settled
# by one of those and the classifier is never asked about a healthy update line by itself. Plant
# `updates_state=good` -> `bad` on the fresh sentence and the whole suite stayed green while the
# alarm told him his taps were going nowhere and quoted, as the evidence, the hub saying it had
# collected some twelve seconds ago.
d=$(case_dir onlythestampstopped); stamp "$d" 3600
note "$d" 10 'not serving' "$PHONE_OK" "$DOOR_OK" "$UPD_OK"
: > "$d/watchdog.armed"; ago > "$d/watchdog.tick"
out=$(wd "$d" --dry-run 2>&1)
printf '%s' "$out" | grep -q 'nothing you tap is reaching an agent' \
  && bad "it said his taps were going nowhere while the hub says it collected some seconds ago: $out" \
  || ok "a healthy update line on its own is not read as the half that failed"
printf '%s' "$out" | grep -q 'phone line is down' \
  && bad "it asserted the phone line was down while the hub says it answered seconds ago: $out" \
  || ok "and an outage it cannot pin on a half does not assert one at random"
printf '%s' "$out" | grep -q 'gone quiet' \
  && ok "it says the herd has gone quiet, which is the whole of what is known" \
  || bad "it said nothing honest about a hub whose stamp stopped with every half reading well: $out"
printf '%s' "$out" | grep -q 'The hub says' \
  && bad "it quoted a half the hub says is healthy as the evidence of the fault: $out" \
  || ok "and quotes no half as evidence, because none of them is the fault"

# Every shape of outage has to fit in the record of which halves have already been told. With two
# halves that record was three words long; with three it is eight shapes, and a record truncated
# mid-word stops matching, which turns the repeat interval off and sends one message a minute.
d=$(case_dir everyshape); : > "$d/watchdog.armed"
for combo in "$PHONE_BAD|$DOOR_OK|$UPD_OK"  "$PHONE_OK|$DOOR_BAD|$UPD_OK" \
             "$PHONE_OK|$DOOR_OK|$UPD_BAD"  "$PHONE_BAD|$DOOR_BAD|$UPD_OK" \
             "$PHONE_BAD|$DOOR_OK|$UPD_BAD" "$PHONE_OK|$DOOR_BAD|$UPD_BAD" \
             "$PHONE_BAD|$DOOR_BAD|$UPD_BAD"; do
  IFS='|' read -r p dr u <<< "$combo"
  ago > "$d/watchdog.tick"; stamp "$d" 3600; note "$d" 10 'not serving' "$p" "$dr" "$u"
  wd "$d" >/dev/null 2>&1
done
# One more check of the LAST shape, which by now has certainly been told once.
ago > "$d/watchdog.tick"; stamp "$d" 3600
note "$d" 10 'not serving' "$PHONE_BAD" "$DOOR_BAD" "$UPD_BAD"
out=$(wd "$d" 2>&1)
printf '%s' "$out" | grep -q 'operator was NOT told' \
  && bad "the record of which halves were told no longer holds every shape, so the repeat interval stopped applying" \
  || ok "the record of which halves were told holds every shape of outage without losing one"

# ── what the watchdog is not allowed to become ───────────────────────────────
# A "reset the state and see" refactor is the realistic way this script starts deleting the record
# of who asked what — while the operator is being told his phone line is down and has no way to
# check. The watchdog owns watchdog.* and nothing else, and it alarms rather than acting.
# Comments are stripped first: this script's header explains at length what it must never do, and a
# guard that reads its own warning as the offence is a guard nobody can keep green.
code() { sed 's/#.*//' "$W"; }
owned='hub\.heartbeat|hub\.health|hub\.generations\.json|hub\.connected\.json|hub\.audit\.log|asks\.json|\$BEAT|\$AGENTS'
if code | grep -nE '\brm\b' | grep -qE "$owned"; then
  bad "the watchdog removes a file the hub owns: $(code | grep -nE '\brm\b' | grep -E "$owned")"
else
  ok "the watchdog never removes a file the hub owns"
fi
if code | grep -qE '\b(systemctl|systemd-run|kill)\b'; then
  bad "the watchdog tries to act on the hub instead of reporting it: $(code | grep -nE '\b(systemctl|systemd-run|kill)\b')"
else
  ok "the watchdog alarms and restarts nothing — restarting is somebody else's job"
fi

printf '\npass=%s fail=%s\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
