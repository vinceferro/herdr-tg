#!/usr/bin/env bash
# herdr-tg-watchdog — the one alarm that must not depend on the thing it watches.
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
#   * Withhold the stamp when ANY of the three legs of the control plane has stopped working — the
#     phone line out to Telegram, the line Telegram sends his taps back down, and the door agents
#     arrive at. "get_me answered" is a third of this product: a hub whose socket never opened
#     answers it every forty-five seconds while every agent on the box talks to nobody, and a hub
#     whose long poll is being held by a second copy of the bot answers it just as cheerfully while
#     every tap the operator makes is delivered to the other copy and dies there. Withholding is the
#     only signal this script can hear, so it is the one used.
#
# WHICH LEG, and why it is a second file. The stamp's modification time cannot say which leg went
# quiet, and the operator's next move differs — a dead phone line is a machine to go and look at, a
# dead door or a held update line is a hub to restart while the phone in his hand keeps working and
# tells him nothing is wrong. So the hub also writes $STATE_DIR/hub.health on EVERY tick, green or
# not: a word, then one plain sentence for the phone line, one for the agents' door, and one for the
# update line. This script reads it ONLY to attribute an alarm it has already decided to raise, never
# to decide whether to raise one — a hub sick enough to be withholding its stamp is exactly the hub
# whose account of itself must not be allowed to overrule the stamp. A hub that writes no note at all
# still alarms, exactly as before, and so does one whose note this script is too old to understand.
#
# WHAT IT WILL NOT DO. It never infers that silence is fine. A clean stop still buzzes: this repo's
# whole failure history is things going quiet in a way that looked healthy, so the watchdog fails
# toward noise. Silencing is explicit, and it expires on its own.
#
# WHAT IT CANNOT DO, written down rather than implied. It runs in the same user manager, under the
# same uid, on the same box as the hub. A dead box, a dead systemd, or a dead network takes both.
# The only alarm that survives those is kickoff's, which carries its own token and its own curl.

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
TICK="$STATE_DIR/watchdog.tick"
ENV_FILE="${HERDR_TG_ENV_FILE:-${XDG_CONFIG_HOME:-$HOME/.config}/herdr-tg/env}"
STALE_AFTER="${HERDR_TG_STALE_AFTER:-180}"      # seconds without a stamp before the hub counts as dead
REALARM_EVERY="${HERDR_TG_REALARM_EVERY:-30}"   # further stale checks between repeat alarms
# Two of the hub's own forty-five-second ticks (`bot::WATCHDOG_TICK`, the constant this is derived
# from — slow that tick down and this must move with it). A hub that is running at all rewrites the
# note every tick, so anything older than this is not a live hub explaining what it is withholding;
# it is what a hub that has since died left behind. Believing it says "your phone still works" when
# there is no bot at all. It was the staleness window, which by definition admits every such note.
NOTE_TRUSTED_FOR="${HERDR_TG_NOTE_TRUSTED_FOR:-90}"
DISARM_EXPIRES="${HERDR_TG_DISARM_EXPIRES:-86400}"  # a silence you forgot about is the failure again

log()  { printf '%s herdr-tg-watchdog: %s\n' "$(date -Is)" "$*" >&2; }
# `~`-form for anything the operator reads: the full path is his home directory, and every alarm
# leaves this machine. Nothing that goes to Telegram needs to carry it.
tilde() { printf '%s' "${1/#$HOME/\~}"; }

usage() {
  cat >&2 <<USAGE
herdr-tg-watchdog — buzz the operator when the hub stops answering.

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
  local RESTART="This is the watchdog, not the bot — it can tell you, it cannot fix it. Restarting herdr-tg is the usual fix."
  # The one cause worth naming: a stray `serve` in a terminal, a unit started twice, a laptop and a
  # server sharing one token. Telegram hands the taps to one copy, and the other — the one with the
  # agents — never sees them. It is not the only way this leg dies (a wedged dispatcher stops looking
  # at all, and the hub says so in its own sentence just above), but it is much the likeliest and the
  # only one he can check in ten seconds. "Either way" is there because the fix is the same for both.
  local SECOND="This is the watchdog, not the bot — it can tell you, it cannot fix it. The likeliest cause is a second copy of this bot taking your taps; restarting herdr-tg is the usual fix either way."
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
  send "$(hostname): herdr-tg watchdog drill. Nothing is wrong. This message only proves the alarm can reach you." \
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

if [ "$gone" = no ]; then
  # Hysteresis: clear the latch only on a comfortably fresh stamp. A hub whose cadence sits right
  # at STALE_AFTER would otherwise flap fresh/stale and alarm on every other check, throttle and
  # all — the latch would be reset each time it was about to do its job.
  if [ "$age" -lt "$(( STALE_AFTER / 2 ))" ]; then
    rm -f "$LATCH" "$LEGS"
    exit 0
  fi
  [ "$age" -lt "$STALE_AFTER" ] && exit 0
  when="$(human_age "$age")"
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
shape=unclear; hub_says=
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
    # Line 4 is the update line's, and it is APPENDED rather than inserted for a reason that cuts
    # both ways: this script is copied into place at install time, so an installed watchdog older
    # than the hub reads lines 2 and 3 and is simply unaffected, while this one reads a three-line
    # note from an older hub as "it says nothing about the update line" — never as a fault.
    said_updates="$(sed -n 4p "$NOTE" 2>/dev/null | tr -d '\000-\037\177' | cut -c1-160)"
    # THE SENTENCES THE HUB CAN WRITE, in this script's own words because it shares no code with the
    # hub — and pinned to the hub's, both ways, by
    # crates/herdr-tg/tests/the_heartbeat_is_earned_not_scheduled.rs, which fails if either side
    # rewords one. A sentence matching NONE of them is a hub newer than this copy of the script (it
    # is copied into place at install time, so hub and watchdog drift apart as a matter of course)
    # and is read as "cannot tell", never as that half being unwell: reading it as unwell reported
    # every outage as total and quoted, as the evidence of the fault, the line saying the half was
    # fine.
    phone_state=unknown; door_state=unknown; updates_state=unknown
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
    # The two lines that have always been on the note must BOTH be readable before anything is
    # named. A note caught with only its first line written, or one whose words this script does not
    # know, says nothing about either half — and naming a half out of that is naming one at random.
    #
    # The update line is deliberately not in that gate. A hub built before it was a leg writes three
    # lines, and a note that says nothing about the fourth half must still name the two it does
    # describe — otherwise upgrading the hub before the installed watchdog is what stops the alarm
    # naming anything. It cannot produce a false all-clear either way: the stopped stamp has already
    # decided that there is an alarm, and this block only chooses the words.
    if [ "$phone_state" != unknown ] && [ "$door_state" != unknown ]; then
      # Built as a list rather than a nested if, because three legs is eight cases and a chain of
      # elifs is where one of them quietly stops being reachable.
      failing=
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
        # Every leg the hub describes reads well while the stamp has stopped. shape stays `unclear`
        # on purpose: the stamp wins, and the honest thing to say is that we cannot tell which.
        *)                     shape=unclear ;;
      esac
      # Quoted in the order the head sentence names them, so the two halves of one message agree.
      [ "$phone_state"   = bad ] && hub_says="The hub says: $said_phone"
      [ "$updates_state" = bad ] && hub_says="${hub_says:+$hub_says
}The hub says: $said_updates"
      [ "$door_state"    = bad ] && hub_says="${hub_says:+$hub_says
}The hub says: $said_door"
    fi
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
# once come to 62 characters. A cap two characters clear of the real worst case is one that starts
# truncating the last shape mid-word the day a ninth is added — and a truncated record matches
# nothing, which turns the repeat interval off and sends one message a minute.
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
