#!/usr/bin/env bash
# kickoff-channel-watchdog — the one alarm that must not depend on the thing it watches.
#
# The FILE keeps its former name because nobody types it: several guards and both scripts read it
# at this path, and the operator meets it as the unit `kickoff-channel-watchdog` and the command
# `~/.local/bin/kickoff-channel-watchdog` that scripts/install-watchdog.sh lays down from it.
#
# The hub is one bot holding the only phone line between the operator and a herd of coding agents.
# If it dies, the phone simply goes quiet — and quiet looks exactly like "nothing is happening".
# This turns that silence into a buzz. It shares no code and no process with the hub: its own
# token read, its own curl, its own timer. Anything borrowed from the hub would die with the hub.
#
# THE HEARTBEAT CONTRACT — the hub must hold up its end, or this is a no-op that exits 0 forever:
#   * Stamp $STATE_DIR/hub.heartbeat while healthy, at least every STALE_AFTER/3 seconds.
#   * Update it IN PLACE. Never delete it, not even on a clean shutdown — a deleted stamp used to
#     read as "the hub never ran", which is how an alarm turns itself off in silence.
#   * Never put it on a tmpfs. A reboot would empty it and disarm the alarm permanently.
#   * Stamp only when the work loop is alive. A process that stamps from a timer while its
#     dispatcher is wedged is a hub that is dead to the operator and healthy to this script.
#   * Withhold the stamp when ANY of the three legs of the control plane has stopped working — what
#     carries an agent's words to him, the door agents arrive at, and what carries his own words
#     back to an agent. "get_me answered" is a third of this product: a hub whose socket never
#     opened answers it every forty-five seconds while every agent on the box talks to nobody, and a
#     hub whose long poll is being held by a second copy of the bot answers it just as cheerfully
#     while every tap the operator makes is delivered to the other copy and dies there. Withholding
#     is the only signal this script can hear, so it is the one used.
#
# TWO PLANES, and both of them stamp. A hub reaches him either through a messaging app or through
# the app he reads on his phone, and it holds three legs either way: on the phone line they are the
# line out to Telegram, the door, and the stream his taps come back down; in the app they are what
# agents say being written down for it to read, the same door, and the sweep of the place his
# answers arrive. An app hub has no phone line and must still EARN a stamp, because this script arms
# on the note as well as on the stamp — see the arming block below — so a plane that never stamped
# would alarm every minute, for ever, on a box where nothing is wrong. That is not a quieter failure
# than a missed alarm; it teaches him to ignore the one message that has to be trusted.
#
# WHICH LEG, and why it is a second file. The stamp's modification time cannot say which leg went
# quiet, and the operator's next move differs — a dead phone line is a machine to go and look at, a
# dead door or a held update line is a hub to restart while the phone in his hand keeps working and
# tells him nothing is wrong. So the hub also writes $STATE_DIR/hub.health on EVERY tick, green or
# not: a word, then one plain sentence per leg, in the order above. This script reads it ONLY to
# attribute an alarm it has already decided to raise, never to decide whether to raise one — a hub
# sick enough to be withholding its stamp is exactly the hub whose account of itself must not be
# allowed to overrule the stamp. A hub that writes no note at all still alarms, exactly as before,
# and so does one whose note this script is too old to understand. Which plane the note came from is
# read off the note's own sentences, and nothing else: the first line is either a phone line's or an
# app copy's, and the two sets of words share nothing.
#
# THE SECOND PROCESS, and the one fact here that is not a file. Every leg of the hub's stamp on the
# app plane is the hub watching itself: it writes the ring, it accepts at the agents' door, it lists
# the place his answers land. The whole of the distance between the operator and those three files
# is a DIFFERENT program — kickoff-door, its own unit, its own binary, its own port, its own
# credential — and the hub cannot witness it without becoming a thing that dials a port, which is
# the one thing it is designed never to be. So on 19 September `serve --to app` was started on a box
# with no kickoff-door binary anywhere: it stamped this file within a minute, with all three legs
# green, and an armed watchdog said nothing at all, for ever, while the app the operator holds was
# dark. That is this system's oldest failure — something answering its own liveness check while
# every real path through it is dead — recurring exactly one process boundary further out.
#
# So the door's own unit is asked about on every check. It is ASKED and never told: is-enabled, then
# the state, and nothing that starts, stops or restarts anything — the same line this script keeps
# with the hub, which it reports and never fixes. Two rules keep it from crying wolf. It is watched
# only where the operator ENABLED it, because his own `systemctl --user enable` is the declaration
# that he depends on it and a box that never had a door is not a box with a broken one. And it is
# watched only once this script is armed on a hub, because a door with no hub to serve is a machine
# mid-setup rather than an outage. Where there is no user manager to ask, nothing is claimed.
#
# WHAT THE DOOR'S UNIT STILL DOES NOT PROVE, written down rather than implied like every other limit
# here: that a door which is RUNNING is serving. A wedged accept loop, a door refusing every write
# because the token was rotated out from under it, a bridge that cannot reach the port — all three
# read `running`. This fact is the difference between "it is dead" and "nobody knows", never between
# "it is dead" and "it is well".
#
# WHAT IT WILL NOT DO. It never infers that silence is fine. A clean stop still buzzes: this repo's
# whole failure history is things going quiet in a way that looked healthy, so the watchdog fails
# toward noise. Silencing is explicit, and it expires on its own.
#
# WHAT IT CANNOT DO, written down rather than implied. It runs in the same user manager, under the
# same uid, on the same box as the hub. A dead box, a dead systemd, or a dead network takes both.
# The only alarm that survives those is kickoff's, which carries its own token and its own curl.
#
# AND ITS ONE CARRIER IS A BOT TOKEN. Everything below decides, words and throttles an alarm; the
# only way it ever leaves this machine is sendMessage. On a box whose plane is the app there is no
# token to find — the hub's own unit deliberately reads no credential file — so `send` says FAILED
# in the journal and this script exits non-zero, which puts the unit in `failed` and fires whatever
# OnFailure= is wired to. That is the whole of what an app box gets, and it is said here rather than
# discovered: the decision, the wording and the throttle are all still right, and a person watching
# `systemctl --user --failed` or a desktop notification is the one who finds out. Giving this script
# a second carrier is a change with its own credential, its own failure modes and its own reasons to
# be got right, and inventing one quietly in the same edit as the plane is how the alarm nobody
# tested becomes the alarm nobody hears.

set -uo pipefail

STATE_DIR="${HERDR_TG_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/herdr-tg}"
BEAT="$STATE_DIR/hub.heartbeat"
NOTE="$STATE_DIR/hub.health"          # the hub's own account of which half failed; never the trigger
ARMED="$STATE_DIR/watchdog.armed"
DISARM="$STATE_DIR/watchdog.disarmed"
LATCH="$STATE_DIR/watchdog.latch"
# Which halves the last alarm named. One counter for "an alarm is in progress" would have let the
# door's half-hourly repeat swallow the first alarm of the phone line going down twenty minutes
# later — the one message that has no second chance.
LEGS="$STATE_DIR/watchdog.legs"
# When the door was last seen down, so that ONE sample of it being up cannot wipe the two files
# above. The third of the same shape on this page: the stamp gets hysteresis, a flapping leg is
# remembered as a set rather than compared with the last one, and a door needs both because a
# single boolean sample of a unit carries no margin at all to be comfortable about.
DOORWENT="$STATE_DIR/watchdog.door"
TICK="$STATE_DIR/watchdog.tick"
ENV_FILE="${HERDR_TG_ENV_FILE:-${XDG_CONFIG_HOME:-$HOME/.config}/herdr-tg/env}"
# The program the operator's app reaches this machine through. Named here because it is the one
# thing this script watches that the hub cannot see at all; watched only if he enabled it. The
# program is reached through PATH and never through a variable — this unit pins PATH to /usr/bin:/bin
# for the same reason it holds a bot token, and a setting naming a program would put that back.
DOOR_UNIT="${HERDR_TG_DOOR_UNIT:-kickoff-door.service}"
STALE_AFTER="${HERDR_TG_STALE_AFTER:-180}"      # seconds without a stamp before the hub counts as dead
REALARM_EVERY="${HERDR_TG_REALARM_EVERY:-30}"   # further stale checks between repeat alarms
# Two of the hub's own forty-five-second ticks (`bot::WATCHDOG_TICK`, the constant this is derived
# from — slow that tick down and this must move with it). A hub that is running at all rewrites the
# note every tick, so anything older than this is not a live hub explaining what it is withholding;
# it is what a hub that has since died left behind. Believing it says "your phone still works" when
# there is no bot at all. It was the staleness window, which by definition admits every such note.
NOTE_TRUSTED_FOR="${HERDR_TG_NOTE_TRUSTED_FOR:-90}"
DISARM_EXPIRES="${HERDR_TG_DISARM_EXPIRES:-86400}"  # a silence you forgot about is the failure again

log()  { printf '%s kickoff-channel-watchdog: %s\n' "$(date -Is)" "$*" >&2; }
# `~`-form for anything the operator reads: the full path is his home directory, and every alarm
# leaves this machine. Nothing that goes to Telegram needs to carry it.
tilde() { printf '%s' "${1/#$HOME/\~}"; }

usage() {
  cat >&2 <<USAGE
kickoff-channel-watchdog — buzz the operator when the hub stops answering.

  (no arguments)   one check. Alarms only if the hub is armed and has gone quiet.
  --test           send one drill message and exit. Proves the alarm can reach the operator.
  --dry-run        decide as usual, print the alarm it would send, send nothing.
  --help           this.
USAGE
}

# An unknown argument used to fall through to a live check, so `--help` sent a real alarm.
MODE=check
case "${1:-}" in
  '')         ;;
  --test)     MODE=test ;;
  --dry-run)  MODE=dry ;;
  --help|-h)  usage; exit 0 ;;
  *)          printf 'unknown argument: %s\n\n' "$1" >&2; usage; exit 2 ;;
esac
[ "$#" -le 1 ] || { printf 'too many arguments\n\n' >&2; usage; exit 2; }

# Config that cannot be trusted is config that alarms about a healthy hub forever. A systemd-shaped
# `3min` is the realistic mistake — every arithmetic comparison below would silently read it as 0.
for v in STALE_AFTER REALARM_EVERY DISARM_EXPIRES NOTE_TRUSTED_FOR; do
  case "${!v}" in
    ''|*[!0-9]*) log "FATAL — $v is not a whole number of seconds; refusing to guess"; exit 78 ;;
    0)           log "FATAL — $v is 0; refusing to guess"; exit 78 ;;
  esac
done

# ── sending ──────────────────────────────────────────────────────────────────
# The token is in the URL, so the URL never reaches argv: curl reads it from a config file on
# stdin. `-q` is load-bearing and easy to miss — without it curl reads ~/.curlrc first, and a
# single `trace-ascii` or `dump-header` line in there would write the URL, token and all, into a
# file this script never chose. The message body goes in via a file for the same reason plus one
# more: it is multi-line, and argv is the wrong place for text with newlines in it.
send() {
  local text="$1" token chat code tmp raw
  if [ ! -r "$ENV_FILE" ]; then
    log "FAILED — no credentials at $(tilde "$ENV_FILE"); the operator was NOT told"
    return 1
  fi
  # systemd's EnvironmentFile= strips quotes and tolerates whitespace, and the hub reads this same
  # file through it. Parsing it any more strictly here means a file that works perfectly for the
  # hub silently breaks only the alarm — so match systemd: drop `export `, trim, drop a trailing
  # CR from a file that has been through a Windows or phone editor, unwrap one pair of quotes.
  unwrap() {
    raw="$(sed -n "s/^[[:space:]]*\(export[[:space:]]\+\)\?$1=//p" "$ENV_FILE" | head -n1)"
    raw="${raw%$'\r'}"
    raw="${raw#"${raw%%[![:space:]]*}"}"
    raw="${raw%"${raw##*[![:space:]]}"}"
    case "$raw" in
      \"*\") raw="${raw#\"}"; raw="${raw%\"}" ;;
      \'*\') raw="${raw#\'}"; raw="${raw%\'}" ;;
    esac
    printf '%s' "$raw"
  }
  token="$(unwrap HERDR_TG_TOKEN)"
  # The first POSITIVE id is the operator's own chat. Deliberate, not incidental: the allowlist
  # also carries the relay supergroup, and an outage alarm posted into a busy group is an alarm
  # that scrolls away. Falls back to the first id of any sign rather than sending nothing.
  chat="$(unwrap HERDR_TG_ALLOWED_CHAT_IDS | tr ',' '\n' | tr -d '[:space:]' | grep -m1 '^[0-9]' \
          || unwrap HERDR_TG_ALLOWED_CHAT_IDS | tr ',' '\n' | tr -d '[:space:]' | grep -m1 '.')"
  if [ -z "$token" ] || [ -z "$chat" ]; then
    log "FAILED — no bot token or no chat id in $(tilde "$ENV_FILE"); the operator was NOT told"
    return 1
  fi
  case "$token" in
    *[[:space:]]*) log "FAILED — the bot token has whitespace in it; curl would truncate the URL"; return 1 ;;
    *:*) ;;
    *)   log "FAILED — the bot token is not in the <digits>:<secret> shape; refusing to send"; return 1 ;;
  esac

  tmp="$(mktemp -d)" || { log "FAILED — no temp dir; the operator was NOT told"; return 1; }
  printf '%s' "$text" > "$tmp/msg"
  code="$(printf 'url = "%s"\n' "https://api.telegram.org/bot${token}/sendMessage" \
    | curl -q -s -o /dev/null -w '%{http_code}' --max-time 10 \
        --data-urlencode "chat_id=${chat}" \
        --data-urlencode "text@$tmp/msg" \
        -K - 2>/dev/null)"
  rm -rf "$tmp"
  case "$code" in ''|*[!0-9]*) code=000 ;; esac
  if [ "$code" = "200" ]; then
    log "delivered (HTTP 200)"
    return 0
  fi
  # The code is the whole diagnosis and it used to be thrown away: 401 is a dead token, 403 is a
  # blocked bot, 400 is a bad chat id, 000 is no network at all. Four different mornings.
  log "FAILED (HTTP $code) — the operator did NOT receive this alarm"
  return 1
}

# ── the door the operator's app reaches this machine through ─────────────────
# Three answers. `unwatched` is the one that claims nothing — no user manager to ask, a unit he
# never enabled, a unit he masked — and it is what every box that does not run a door gets, so
# nothing here can alarm about a door that was never part of the machine.
#
# `active` alone is NOT enough and this is the subtlety the whole check turns on: kickoff-door.service
# sets Restart=always with no start limit, on purpose, so a door crash-looping on a binary that is
# not there sits in `activating (auto-restart)` for ever and never once reaches `failed`. A missing
# binary after a partial install is precisely the shape this was written for, and `is-failed` would
# have called it healthy for the life of the box.
#
# Both properties are read by NAME out of one `show`, rather than by the order they come back in: a
# pair read positionally is a pair that starts reporting the substate as the state the day systemd
# reorders its output, and the two words are drawn from overlapping vocabularies.
the_door_unit_is() {
  local enabled="" props="" state="" sub=""
  command -v systemctl >/dev/null 2>&1 || { printf unwatched; return; }
  enabled="$(systemctl --user is-enabled "$DOOR_UNIT" 2>/dev/null)"
  case "$enabled" in
    enabled|enabled-runtime|linked|linked-runtime|static) ;;
    *) printf unwatched; return ;;
  esac
  props="$(systemctl --user show -p ActiveState -p SubState "$DOOR_UNIT" 2>/dev/null)"
  state="$(printf '%s\n' "$props" | sed -n 's/^ActiveState=//p' | head -n1)"
  sub="$(printf '%s\n' "$props" | sed -n 's/^SubState=//p' | head -n1)"
  # Anything this script could not read is `stopped`, not `unwatched`: he has told the machine he
  # depends on this door, and "I asked and could not tell" is a reason to speak, not to go quiet.
  if [ "$state" = active ] && [ "$sub" = running ]; then printf serving; else printf stopped; fi
}

# "4 hours 12 minutes", not "252". The one message that has to be trusted should not make the
# operator do arithmetic at 3am.
human_age() {
  local s="$1" d=$(( $1 / 86400 )) h=$(( ($1 % 86400) / 3600 )) m=$(( ($1 % 3600) / 60 ))
  if   [ "$s" -lt 90 ];    then printf 'under a minute ago'
  elif [ "$d" -gt 0 ];     then printf '%d day(s) %d hour(s) ago' "$d" "$h"
  elif [ "$h" -gt 0 ];     then printf '%d hour(s) %d minute(s) ago' "$h" "$m"
  else                          printf '%d minute(s) ago' "$m"; fi
}

# Which legs are down decides the words, because it decides what he has to do. `unclear` gets its
# own arm and names no half at all: a hub that died outright, one built before it said anything, and
# one whose stamp stopped while every half it describes reads well are all the same picture, and the
# honest thing to say about it is that we cannot tell. It used to share the phone line's arm, which
# turned "we cannot tell" into "your phone line is down" — asserted beside a note, quoted or not,
# saying the phone line had answered seconds ago. A guess dressed as a diagnosis is worse than the
# general truth, and this is the one message that has to be trusted.
#
# Written out rather than composed from three clauses, because every sentence he can receive should
# be readable here, whole, without running the script in your head. Eight arms for eight shapes.
# The three closings differ by what he has to go and do, which is the whole reason to name a leg.
alarm_text() {
  local head body close
  local SILENT="This is the watchdog, not the bot — it can tell you, it cannot fix it. Someone has to look at the machine."
  local RESTART="This is the watchdog, not the bot — it can tell you, it cannot fix it. Restarting kickoff-channel is the usual fix."
  # The one cause worth naming: a stray `serve` in a terminal, a unit started twice, a laptop and a
  # server sharing one token. Telegram hands the taps to one copy, and the other — the one with the
  # agents — never sees them. It is not the only way this leg dies (a wedged dispatcher stops looking
  # at all, and the hub says so in its own sentence just above), but it is much the likeliest and the
  # only one he can check in ten seconds. "Either way" is there because the fix is the same for both.
  local SECOND="This is the watchdog, not the bot — it can tell you, it cannot fix it. The likeliest cause is a second copy of this bot taking your taps; restarting kickoff-channel is the usual fix either way."
  # The app's own closing. "Not the bot" would name something that does not exist on a box whose
  # hub holds no credential and dials no messaging service, and a second copy of it is not a cause
  # to point at either: one hub lock and one socket mean the second copy never starts at all.
  local APP_RESTART="This is the watchdog, not the hub — it can tell you, it cannot fix it. Restarting the hub is the usual fix."
  # The door's own closing, and it names a different program on purpose: restarting the hub here
  # fixes nothing and costs him every agent on the box a reconnect for a fault the hub does not have.
  local DOOR_RESTART="This is the watchdog, not the hub — it can tell you, it cannot fix it. Restarting kickoff-door is the usual fix; the hub itself needs nothing."
  case "$shape" in
    door)
      head="$(hostname): the herd's phone line is up, but agents cannot reach it."
      body="The bot is still answering Telegram. No agent can ask you anything, and nothing you tap reaches an agent."
      close="$RESTART" ;;
    updates)
      head="$(hostname): the herd's phone line is up, but nothing you tap is reaching an agent."
      body="The bot is still answering Telegram, and your taps and typed lines are not getting from there to an agent. They are going nowhere, in silence."
      close="$SECOND" ;;
    door+updates)
      head="$(hostname): the herd's phone line is up, but nothing you tap is reaching an agent, and agents cannot reach it either."
      body="The bot is still answering Telegram. Nothing else between you and the agents on this machine is working."
      close="$SECOND" ;;
    both)
      head="$(hostname): the herd's phone line is down, and agents cannot reach it either."
      body="Nothing you send reaches an agent, and nothing an agent says reaches you."
      close="$SILENT" ;;
    phone+updates)
      head="$(hostname): the herd's phone line is down, and nothing you tap is reaching an agent."
      body="Nothing you send reaches an agent, and nothing an agent says reaches you."
      close="$SILENT" ;;
    all)
      head="$(hostname): the herd's phone line is down, nothing you tap is reaching an agent, and agents cannot reach it either."
      body="Nothing about the control plane is working."
      close="$SILENT" ;;
    phone)
      head="$(hostname): the herd's phone line is down."
      body="Nothing you send reaches an agent, and nothing an agent says reaches you."
      close="$SILENT" ;;
    # ── the app's seven, in the same order and for the same reason: written out whole, so that
    # every sentence he can receive is readable here without running the script in your head.
    app-ring)
      head="$(hostname): the herd is running, but nothing it says is reaching your app."
      body="Agents can still reach this machine, and the answers you have already given still get back to them. Nothing new they say is being written down for your app, so you will not see any of it."
      close="$APP_RESTART" ;;
    app-sweep)
      head="$(hostname): the herd is running, but nothing you tap is reaching an agent."
      body="Agents can still reach this machine and what they say still reaches your app. Your taps and typed lines are not being collected, so they are going nowhere, in silence."
      close="$APP_RESTART" ;;
    app-door)
      head="$(hostname): agents cannot reach the herd."
      body="No agent can ask you anything, and nothing you tap reaches an agent."
      close="$APP_RESTART" ;;
    app-ring+sweep)
      head="$(hostname): agents can reach the herd, and nothing is getting through to you or back from you."
      body="Agents are still connecting to this machine. Nothing they say is reaching your app, and nothing you tap is reaching them."
      close="$APP_RESTART" ;;
    app-ring+door)
      head="$(hostname): agents cannot reach the herd, and nothing it says is reaching your app either."
      body="Nothing you send reaches an agent, and nothing an agent says reaches you."
      close="$APP_RESTART" ;;
    app-door+sweep)
      head="$(hostname): agents cannot reach the herd, and nothing you tap is reaching one either."
      body="Nothing you send reaches an agent, and nothing an agent says reaches you."
      close="$APP_RESTART" ;;
    app-all)
      head="$(hostname): nothing between you and the agents on this machine is working."
      body="Nothing about the control plane is working."
      close="$APP_RESTART" ;;
    # The one shape the hub's own stamp can never be evidence about, which is why it is the only
    # shape here reached while that stamp is perfectly fresh. It says so out loud: a man who has
    # just checked the hub and found it healthy needs to be told that is not the question.
    app-door-process)
      head="$(hostname): the herd is running, but the door your app reaches it through has stopped."
      body="Agents are still working and the hub is still answering for itself. Nothing it says is reaching your app and nothing you tap there is reaching an agent. The hub cannot see this and will go on looking perfectly healthy for as long as it lasts, which is why you are hearing it from something else."
      close="$DOOR_RESTART" ;;
    *)
      head="$(hostname): the herd has gone quiet, and it is not saying which part stopped."
      body="Nothing you send reaches an agent, and nothing an agent says reaches you."
      close="$SILENT" ;;
  esac
  printf '%s\n\n%s\n%s\n' "$head" "$body" "Last sign of life: $1"
  [ -n "$hub_says" ] && printf '%s\n' "$hub_says"
  printf '\n%s\n%s\n' "$close" "To stay quiet for a day:  touch $(tilde "$DISARM")"
}

if [ "$MODE" = test ]; then
  send "$(hostname): kickoff-channel watchdog drill. Nothing is wrong. This message only proves the alarm can reach you." \
    && exit 0 || exit 1
fi

mkdir -p "$STATE_DIR" 2>/dev/null

# ── the disarm, which expires on its own ─────────────────────────────────────
if [ -e "$DISARM" ]; then
  d_age=$(( $(date +%s) - $(stat -c %Y "$DISARM" 2>/dev/null || echo 0) ))
  if [ "$d_age" -lt "$DISARM_EXPIRES" ] && [ "$d_age" -ge 0 ]; then
    exit 0
  fi
  # A permanent off switch is how an alarm dies quietly, so this one wears off. Removing the file
  # rather than warning about it means the next check behaves exactly like a normal one.
  rm -f "$DISARM"
  log "the disarm expired after $DISARM_EXPIRES s — watching again"
fi

# ── suspend, reboot, and a stopped timer all look like a stale hub ───────────
# The stamp ages on the wall clock; the timer counts monotonic seconds that stop while the laptop
# sleeps. Close the lid for four hours and the first check after resume sees a four-hour-old stamp
# from a hub that is perfectly healthy and has simply not been scheduled yet. Measuring the gap
# BETWEEN checks catches all three cases without needing a monotonic clock in shell: a gap far
# larger than the timer interval means this script was not running, whatever the reason.
now=$(date +%s)
prev=0; grace=0
if [ -r "$TICK" ]; then read -r prev grace < "$TICK" 2>/dev/null || { prev=0; grace=0; }; fi
case "$prev" in ''|*[!0-9]*) prev=0 ;; esac
case "$grace" in ''|*[!0-9]*) grace=0 ;; esac
if [ "$prev" -eq 0 ] || [ "$(( now - prev ))" -gt "$STALE_AFTER" ] || [ "$now" -lt "$prev" ]; then
  grace=$(( now + STALE_AFTER ))     # give the hub one full window to stamp before judging it
fi
printf '%s %s\n' "$now" "$grace" > "$TICK" 2>/dev/null
if [ "$now" -lt "$grace" ]; then
  exit 0
fi

# ── armed, or not yet ────────────────────────────────────────────────────────
# Absence of the stamp used to mean one thing: "the hub has never run". It also means "the stamp
# was deleted", which is what a tidy shutdown, a wiped state dir, or a one-character path mismatch
# produce — and all three turned the alarm off forever while looking exactly like today's correct
# silence. So the watchdog remembers, once, that it has seen a hub alive here.
if [ -e "$BEAT" ]; then
  [ -e "$ARMED" ] || { : > "$ARMED"; log "armed — a hub has stamped here; a missing stamp is now an alarm"; }
  beat=$(stat -c %Y "$BEAT" 2>/dev/null) || beat=0
  age=$(( now - beat ))
  # A stamp from the future is a clock that moved, not a healthy hub. Reading it as "fresh" blinds
  # the watchdog for the whole size of the skew, so treat any negative age as unknown, not good.
  [ "$age" -lt 0 ] && age=0 && log "the stamp is in the future — clock skew; treating it as just-now"
  gone=no
elif [ -e "$NOTE" ]; then
  # A hub has run here and has NEVER earned a stamp. Arming on the stamp alone left exactly this box
  # silent for its whole life — a socket that would not bind, or no forum configured, means the hub
  # answers Telegram every forty-five seconds and withholds every stamp, which is the outage the
  # two-legged stamp exists to catch and the one it could not report. The hub writes its note on
  # every tick whichever way the verdict went, so the note is the proof that a hub has run here.
  [ -e "$ARMED" ] || { : > "$ARMED"; log "armed — a hub has run here and never earned a stamp"; }
  age=-1; gone=never
elif [ -e "$ARMED" ]; then
  age=-1; gone=yes
else
  exit 0                               # never armed: today's state, and it must stay silent
fi

# ── the door the app reads this machine through ──────────────────────────────
# Asked here, past the disarm and past the resume window and inside the arming that every other
# judgement is inside, but BEFORE the stamp is judged — because this is the one fact on this page
# that a fresh stamp is not evidence about. A hub with a dead door stamps a green file every
# forty-five seconds; see the header.
door="$(the_door_unit_is)"
door_says=
[ "$door" = stopped ] && door_says="And the door your app reaches this machine through has stopped as well."

# How long the door has been up EVERY time this script has looked, which is not the same question as
# whether it is up right now — and the throttle below depends on the difference. kickoff-door.service
# sets Restart=always with RestartSec=5 and no start limit, deliberately, so a door that binds and
# dies tens of seconds later (a token rotated out from under it, a panic on a request, an OOM) is
# `running` when half the checks look and `dead` when the other half do. Reading the instant alone
# cleared the latch and the record of what had been told on every up-sample, and the next check that
# caught it down started the repeat interval from zero: six alarms in twelve checks, measured on this
# script, against one in ten for a door that simply stayed down.
#
# So the door is given the same margin the stamp has: it counts as recovered only once it has been
# up on every check across half the staleness window, which is more than one check apart at any
# sane timer interval. A clock that moved backwards reads as "seen down just now" rather than as a
# long recovery — the quiet way to be wrong here, because the set of halves already told still
# starts the throttle again the moment a DIFFERENT shape appears.
door_up_for=$(( STALE_AFTER * 2 ))   # nothing remembered: this door has never been seen down here
door_went=
# No `|| door_went=` on the read, unlike the tick file above, and the difference is deliberate: a
# `read` that hits end of file without a newline returns non-zero having set the variable anyway,
# and the tick file resets to a value that gives the hub MORE room while this one would reset to
# "never seen down", which is the noisy answer. The validation below is what rejects a torn file.
[ -r "$DOORWENT" ] && read -r door_went < "$DOORWENT" 2>/dev/null
case "$door_went" in
  ''|*[!0-9]*) ;;                    # nothing readable is "never seen down", as it was before this
  *) door_up_for=$(( now - door_went )); [ "$door_up_for" -lt 0 ] && door_up_for=0 ;;
esac
if [ "$door" = stopped ]; then
  door_up_for=0
  # Written on every check that finds it down, including a --dry-run: the tick file beside it is
  # written the same way, and a dry run that decided differently from the real check it is there to
  # preview would be worse than the side effect.
  printf '%s\n' "$now" > "$DOORWENT" 2>/dev/null
fi

if [ "$gone" = no ]; then
  # Hysteresis: clear the latch only on a comfortably fresh stamp. A hub whose cadence sits right
  # at STALE_AFTER would otherwise flap fresh/stale and alarm on every other check, throttle and
  # all — the latch would be reset each time it was about to do its job.
  #
  # And only while nothing ELSE is wrong. A stopped door is an outage in progress, and clearing the
  # record of what has already been told in the middle of one is how a half-hourly repeat becomes
  # one message a minute.
  #
  # `door_up_for` rather than `door` alone, for exactly the reason the stamp is held to half the
  # window rather than to the whole of it: a door caught up on one check is a door that may have
  # been down on the last one and down again on the next. Both conditions are spelt out, because
  # with a one-second STALE_AFTER the window is zero and the age test alone would pass while the
  # door was down.
  if [ "$age" -lt "$(( STALE_AFTER / 2 ))" ] && [ "$door" != stopped ]; then
    # The door's margin clears the DOOR's shape and nothing else. Gating the whole record on it
    # was worse than the flap it was written for: while a door swung, no leg's record ever
    # cleared, so a hub outage that had recovered and come back was matched against a set that
    # still held it and went unsaid for a whole repeat interval — measured at five checks, and it
    # is the "cleared with the latch, so a recovery starts the next outage with nothing told"
    # promise two screens down quietly stopping being true. A leg that recovered is news again
    # whatever the door is doing; the door's own shape is the only one a single up-sample cannot
    # vouch for.
    kept=
    if [ "$door_up_for" -lt "$(( STALE_AFTER / 2 ))" ]; then
      told_now=
      [ -r "$LEGS" ] && told_now="$(tr -d '\000-\037\177' < "$LEGS" 2>/dev/null | cut -c1-160)"
      case " $told_now " in *' app-door-process '*) kept='app-door-process' ;; esac
    fi
    if [ -n "$kept" ]; then
      # The latch stays with it: the door's repeat interval is the one thing still running.
      printf '%s\n' "$kept" > "$LEGS" 2>/dev/null
    else
      rm -f "$LATCH" "$LEGS"
    fi
    exit 0
  fi
  if [ "$age" -lt "$STALE_AFTER" ]; then
    [ "$door" != stopped ] && exit 0
    # The hub is stamping normally and the SECOND process is the one that stopped. "Last sign of
    # life" is still answered honestly — there is nothing wrong with the hub's signs of life, and
    # saying so is the fact that tells him where NOT to look.
    when="the hub itself is stamping normally; it is the door that stopped"
  else
    when="$(human_age "$age")"
  fi
elif [ "$gone" = never ]; then
  # NOT "the hub has never once served". There is no stamp here and a hub is writing its note, which
  # is the same picture whether it never earned one or somebody removed the one it had — a tidied
  # state dir, a path that moved. Saying "never" about a hub that served all week is the kind of
  # false certainty that sends him looking for a setup mistake instead of the outage he has.
  when="no stamp at all; the hub cannot say it is serving"
else
  when="the hub's stamp has been deleted"
fi

# ── which half went quiet ────────────────────────────────────────────────────
# The stamp is withheld when either half stops working, so by here we know something is wrong and
# not which thing. This is the only place the note is read, and it changes nothing but the wording.
shape=unclear; hub_says=; plane=unknown
if [ -r "$NOTE" ]; then
  note_at=$(stat -c %Y "$NOTE" 2>/dev/null) || note_at=0
  note_age=$(( now - note_at ))
  # The note is rewritten on every tick. One as old as the stamp is not a live hub explaining what
  # it is withholding — it is the last thing a hub that has since died managed to say, and it names
  # the half that stopped first rather than the fact that the whole process is gone.
  if [ "$note_age" -ge 0 ] && [ "$note_age" -lt "$NOTE_TRUSTED_FOR" ]; then
    # Control characters stripped and the length capped. This is the one place the watchdog repeats
    # another process's text to Telegram, and it must not become a way to put arbitrary bytes there.
    said_phone="$(sed -n 2p "$NOTE" 2>/dev/null | tr -d '\000-\037\177' | cut -c1-160)"
    said_door="$(sed -n 3p "$NOTE" 2>/dev/null | tr -d '\000-\037\177' | cut -c1-160)"
    # The SAME two lines, read a second time as the app's own halves. Line 2 is his phone line on
    # one plane and the app's copy of what agents say on the other; line 3 is the agents' door
    # either way and is read once. Two readings of one line rather than one block that knows both
    # sets of words, because which of the two recognises it is how this script works out which
    # plane the hub is on — and because the pairs (hub sentence, script pattern) are held together
    # one half at a time by crates/kickoff-channel/tests/the_heartbeat_is_earned_not_scheduled.rs.
    said_ring="$said_phone"
    # Line 4 is the update line's, and it is APPENDED rather than inserted for a reason that cuts
    # both ways: this script is copied into place at install time, so an installed watchdog older
    # than the hub reads lines 2 and 3 and is simply unaffected, while this one reads a three-line
    # note from an older hub as "it says nothing about the update line" — never as a fault.
    said_updates="$(sed -n 4p "$NOTE" 2>/dev/null | tr -d '\000-\037\177' | cut -c1-160)"
    said_sweep="$said_updates"
    # THE SENTENCES THE HUB CAN WRITE, in this script's own words because it shares no code with the
    # hub — and pinned to the hub's, both ways, by
    # crates/kickoff-channel/tests/the_heartbeat_is_earned_not_scheduled.rs, which fails if either side
    # rewords one. A sentence matching NONE of them is a hub newer than this copy of the script (it
    # is copied into place at install time, so hub and watchdog drift apart as a matter of course)
    # and is read as "cannot tell", never as that half being unwell: reading it as unwell reported
    # every outage as total and quoted, as the evidence of the fault, the line saying the half was
    # fine.
    phone_state=unknown; door_state=unknown; updates_state=unknown
    ring_state=unknown; sweep_state=unknown
    case "$said_phone" in
      'the phone line answered '*)                        phone_state=good ;;
      'the phone line last answered '*)                   phone_state=bad ;;
      'the phone line did not answer when the hub'*)      phone_state=bad ;;
      'the phone line has not answered since'*)           phone_state=bad ;;
    esac
    case "$said_door" in
      "the agents' door let a connection through "*)      door_state=good ;;
      "the agents' door last let a connection through "*) door_state=bad ;;
      "the agents' door has let nothing through since"*)  door_state=bad ;;
      "the agents' door could not be opened"*)            door_state=bad ;;
      "the agents' door is not there any more"*)          door_state=bad ;;
      "nothing is answering at the agents' door"*)        door_state=bad ;;
      'no forum is configured'*)                          door_state=bad ;;
      'the forum this hub was pointed at'*)               door_state=bad ;;
    esac
    case "$said_updates" in
      'the hub looked for your taps '*)                   updates_state=good ;;
      'the hub last looked for your taps '*)              updates_state=bad ;;
      'the hub has not looked for your taps since'*)      updates_state=bad ;;
      'another copy of this bot is taking your taps'*)    updates_state=bad ;;
      'something has pointed this bot at a web address'*) updates_state=bad ;;
      'the hub is being turned away when it goes'*)       updates_state=bad ;;
    esac
    # The app's two, on the same two lines. A hub on the phone line writes none of these words and
    # a hub in the app writes none of the two blocks above, which is what makes the pair of blocks
    # a plane test as well as a classification.
    case "$said_ring" in
      'what agents say is being written down'*)           ring_state=good ;;
      'what agents say cannot be written down'*)          ring_state=bad ;;
      'the hub has not begun writing down'*)              ring_state=bad ;;
    esac
    case "$said_sweep" in
      'the hub went to collect your answers '*)           sweep_state=good ;;
      'the hub last went to collect your answers '*)      sweep_state=bad ;;
      'the hub has not gone to collect your answers'*)    sweep_state=bad ;;
      'the place your answers arrive could not be made'*) sweep_state=bad ;;
      'the place your answers arrive could not be read'*) sweep_state=bad ;;
    esac
    # Which way this hub reaches him, decided by nothing but which of the two blocks above knew the
    # words on line 2. Not a setting and not a guess: the sentence he is about to be quoted is the
    # same sentence that chose the vocabulary it is quoted in, so the two can never disagree.
    if   [ "$phone_state" != unknown ]; then plane=phone
    elif [ "$ring_state"  != unknown ]; then plane=app
    fi
    # The door's line and the plane's own first line must BOTH be readable before anything is
    # named. A note caught with only its first line written, or one whose words this script does
    # not know, says nothing about either half — and naming a half out of that is naming one at
    # random. "The plane's own first line" is the same gate the phone line used to be, widened by
    # exactly the amount that a second plane exists: an unknown first line is now an unknown PLANE,
    # which is at least as good a reason to say nothing.
    #
    # The third line is deliberately not in that gate. A hub built before it was a leg writes three
    # lines, and a note that says nothing about the third half must still name the two it does
    # describe — otherwise upgrading the hub before the installed watchdog is what stops the alarm
    # naming anything. It cannot produce a false all-clear either way: the stopped stamp has already
    # decided that there is an alarm, and this block only chooses the words.
    if [ "$plane" != unknown ] && [ "$door_state" != unknown ]; then
      # Built as a list rather than a nested if, because three legs is eight cases and a chain of
      # elifs is where one of them quietly stops being reachable. The app's shapes wear their own
      # names rather than sharing the phone's: every arm of alarm_text differs by what he has to go
      # and do, and "your phone line is down" on a box with no phone line is the guess dressed as a
      # diagnosis that `unclear` exists to avoid.
      failing=
      if [ "$plane" = phone ]; then
        [ "$phone_state"   = bad ] && failing="${failing}phone "
        [ "$updates_state" = bad ] && failing="${failing}updates "
        [ "$door_state"    = bad ] && failing="${failing}door "
        case "$failing" in
          'phone ')              shape=phone ;;
          'updates ')            shape=updates ;;
          'door ')               shape=door ;;
          'phone door ')         shape=both ;;
          'phone updates ')      shape=phone+updates ;;
          'updates door ')       shape=door+updates ;;
          'phone updates door ') shape=all ;;
          # Every leg the hub describes reads well while the stamp has stopped. shape stays
          # `unclear` on purpose: the stamp wins, and the honest thing to say is that we cannot
          # tell which.
          *)                     shape=unclear ;;
        esac
        # Quoted in the order the head sentence names them, so the two halves of one message agree.
        [ "$phone_state"   = bad ] && hub_says="The hub says: $said_phone"
        [ "$updates_state" = bad ] && hub_says="${hub_says:+$hub_says
}The hub says: $said_updates"
        [ "$door_state"    = bad ] && hub_says="${hub_says:+$hub_says
}The hub says: $said_door"
      else
        [ "$ring_state"  = bad ] && failing="${failing}ring "
        [ "$sweep_state" = bad ] && failing="${failing}sweep "
        [ "$door_state"  = bad ] && failing="${failing}door "
        case "$failing" in
          'ring ')             shape=app-ring ;;
          'sweep ')            shape=app-sweep ;;
          'door ')             shape=app-door ;;
          'ring door ')        shape=app-ring+door ;;
          'ring sweep ')       shape=app-ring+sweep ;;
          'sweep door ')       shape=app-door+sweep ;;
          'ring sweep door ')  shape=app-all ;;
          *)                   shape=unclear ;;
        esac
        [ "$ring_state"  = bad ] && hub_says="The hub says: $said_ring"
        [ "$sweep_state" = bad ] && hub_says="${hub_says:+$hub_says
}The hub says: $said_sweep"
        [ "$door_state"  = bad ] && hub_says="${hub_says:+$hub_says
}The hub says: $said_door"
      fi
    fi
  fi
fi

# ── the door, which the hub's own note can never be evidence about ───────────
# Placed after the note is read so it can overrule what the note produced, and it overrules in one
# direction only. Where the hub's stamp is still fresh, a stopped door is the whole of what is
# wrong and it gets the message to itself — that is the state the hub is blind to and the reason
# this fact is collected at all. Where the stamp has stopped too, the hub's own shape stands and
# the door is named BESIDE it: two things broken must never arrive as one of them, and the hub's
# outage is the one that decides where he goes first.
if [ "$door" = stopped ]; then
  if [ "$gone" = no ] && [ "$age" -lt "$STALE_AFTER" ]; then
    shape=app-door-process
    # And nothing is quoted from the hub. Its stamp is inside its own window, so by its own rule it
    # has not earned an alarm yet; a leg its note happens to be grumbling about would arrive here as
    # a diagnosis the stamp has not made, ninety seconds early, and only because the door went down
    # beside it. When that leg really does take the stamp down, it alarms then, in its own shape,
    # which the record of what has been told keeps separate from this one.
    hub_says=
  else
    # Not prefixed "The hub says", because the hub said nothing: it cannot see this program at all.
    hub_says="${hub_says:+$hub_says
}$door_says"
  fi
fi

# ── stale: alarm, bounded ────────────────────────────────────────────────────
n=0
[ -r "$LATCH" ] && n=$(cat "$LATCH" 2>/dev/null)
case "$n" in ''|*[!0-9]*) n=0 ;; esac

# A repeat is only a repeat if it is about the same halves. A half this outage has not named yet —
# the door was down, now the phone line is down too — starts the throttle again, because that is
# news. What is remembered is the SET of halves already told, not the last one: comparing against
# the last one made any CHANGE news, including a change back, so one continuous outage whose other
# half was flapping (a laptop roaming wifi while the door stayed shut) reset the throttle on every
# other check and sent one message a minute all night. This file is cleared with the latch, so a hub
# that recovers starts the next outage with nothing told.
# The cap bounds the file, nothing more — it is this script's own writing, not the hub's. It was 64
# when there were two legs and three shapes; with three legs there are eight, and all eight named at
# once come to 62 characters. A box runs one plane at a time, so the app's eight are the other worst
# case, and with the door's own shape beside them they come to 104. A cap two characters clear of the
# real worst case is one that starts truncating the last shape mid-word the day another is added —
# and a truncated record matches nothing, which turns the repeat interval off and sends one message a
# minute.
told=
[ -r "$LEGS" ] && told="$(tr -d '\000-\037\177' < "$LEGS" 2>/dev/null | cut -c1-160)"
case " $told " in *" $shape "*) ;; *) n=0 ;; esac

if [ "$MODE" = dry ]; then
  printf '%s\n' "--- would send ---"; alarm_text "$when"; printf '%s\n' "--- end ---"
  exit 0
fi

if [ "$n" -ne 0 ] && [ "$n" -lt "$REALARM_EVERY" ]; then
  printf '%s\n' "$(( n + 1 ))" > "$LATCH" 2>/dev/null
  exit 0
fi

# The latch is written BEFORE the send and atomically, because it is the only thing standing
# between one message every thirty minutes and one every sixty seconds. A truncating `>` that hit
# ENOSPC left a zero-byte file, which read back as "never alarmed" — the maximum-noise value. And
# a full disk is not a hypothetical here: it is one of the likelier reasons the hub died at all.
latched=yes
if ! { printf '1\n' > "$LATCH.tmp" && mv -f "$LATCH.tmp" "$LATCH"; } 2>/dev/null; then
  latched=no
  rm -f "$LATCH.tmp" 2>/dev/null
  log "cannot write $(tilde "$LATCH") — repeat suppression is OFF (disk full?)"
fi
# Appended beside the count, so the next check can tell a repeat of this outage from a new one. The
# whole set, so a half that comes and goes is told once rather than on every swing.
case " $told " in *" $shape "*) ;; *) told="${told:+$told }$shape" ;; esac
printf '%s\n' "$told" > "$LEGS" 2>/dev/null

text="$(alarm_text "$when")"
[ "$latched" = no ] && text="$text
(I cannot write my own state, so this may repeat every minute. The disk is probably full.)"

if send "$text"; then
  [ "$latched" = yes ] && exit 0
  exit 1                               # delivered, but the unit must show that it is degraded
fi

# Exiting 0 here is what made a permanently deaf alarm look healthy in `systemctl list-timers` for
# as long as nobody read the journal. A non-zero exit puts the unit in `failed`, which is the one
# place an operator would think to look — and OnFailure= reaches him by a route that is not this bot.
exit 1
