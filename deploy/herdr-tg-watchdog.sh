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
ARMED="$STATE_DIR/watchdog.armed"
DISARM="$STATE_DIR/watchdog.disarmed"
LATCH="$STATE_DIR/watchdog.latch"
TICK="$STATE_DIR/watchdog.tick"
ENV_FILE="${HERDR_TG_ENV_FILE:-${XDG_CONFIG_HOME:-$HOME/.config}/herdr-tg/env}"
STALE_AFTER="${HERDR_TG_STALE_AFTER:-180}"      # seconds without a stamp before the hub counts as dead
REALARM_EVERY="${HERDR_TG_REALARM_EVERY:-30}"   # further stale checks between repeat alarms
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
for v in STALE_AFTER REALARM_EVERY DISARM_EXPIRES; do
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

alarm_text() {
  printf '%s\n\n%s\n%s\n\n%s\n%s\n' \
    "$(hostname): the herd's phone line is down." \
    "Nothing you send reaches an agent, and nothing an agent says reaches you." \
    "Last sign of life: $1" \
    "This is the watchdog, not the bot — it can tell you, it cannot fix it. Someone has to look at the machine." \
    "To stay quiet for a day:  touch $(tilde "$DISARM")"
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
    rm -f "$LATCH"
    exit 0
  fi
  [ "$age" -lt "$STALE_AFTER" ] && exit 0
  when="$(human_age "$age")"
else
  when="the hub's stamp has been deleted"
fi

# ── stale: alarm, bounded ────────────────────────────────────────────────────
n=0
[ -r "$LATCH" ] && n=$(cat "$LATCH" 2>/dev/null)
case "$n" in ''|*[!0-9]*) n=0 ;; esac

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
