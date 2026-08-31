#!/usr/bin/env bash
# Install the herdr-tg watchdog — and PROVE it, so it is not a watchdog you are guessing about.
#
#     bash scripts/install-watchdog.sh          # install, prove the decision, prove the send
#     bash scripts/install-watchdog.sh --dry    # install only; send nothing
#     bash scripts/install-watchdog.sh --fire   # also send ONE real outage alarm for a staged hub
#
# A watchdog can install cleanly, sit in the timer list looking healthy, and still be unable to
# reach anybody — a rotated token, a chat id that was never allowed, a sandbox that blocks DNS.
# All of those look identical from `systemctl list-timers`. So this proves two separate things:
#
#   1. THE DECISION — a staged, hour-old stamp really does produce an alarm. Runs --dry-run, so
#      nothing is sent and nothing lies to you about an outage that is not happening.
#   2. THE SEND — one message that says plainly it is a drill, sent through a transient unit built
#      from the INSTALLED unit's own sandbox directives, so PATH, the address families and the
#      environment are the ones the timer will really use, not a bare shell's.
#
# An earlier version of this script proved neither honestly: it fired a real outage alarm for a
# hub that does not exist, from outside the sandbox, and told the operator to silence it by
# touching a path inside a temp directory that was deleted seconds later.
#
# Idempotent: re-running reinstalls and re-proves.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
BIN_DST="$HOME/.local/bin/herdr-tg-watchdog"
ENV_FILE="$HOME/.config/herdr-tg/env"
STATE_DIR="$HOME/.local/state/herdr-tg"
PROVE=yes; FIRE=no
case "${1:-}" in
  --dry)  PROVE=no ;;
  --fire) FIRE=yes ;;
  '')     ;;
  *)      printf 'usage: install-watchdog.sh [--dry|--fire]\n' >&2; exit 2 ;;
esac

say() { printf '%s\n' "$*"; }
die() { printf '\nerror: %s\n' "$*" >&2; exit 1; }

# An agent session on this box inherits TMPDIR as the literal string "%h/.cache/tmp" — an
# unexpanded systemd specifier — so mktemp hands back a RELATIVE path. CLAUDE.md documents this for
# cargo; it bites shell just as hard, and here it made the staged proof print a nonsense path.
case "${TMPDIR:-/tmp}" in
  /*) ;;
  *)  TMPDIR=/tmp; export TMPDIR ;;
esac

say "herdr-tg watchdog — install and prove"
say "──────────────────────────────────────────────────────────────────"

for f in deploy/herdr-tg-watchdog.sh deploy/herdr-tg-watchdog.service \
         deploy/herdr-tg-watchdog-failed.service deploy/herdr-tg-watchdog.timer; do
  [ -f "$REPO/$f" ] || die "missing $f"
done
command -v systemctl >/dev/null || die "systemctl not found — this needs systemd"
command -v curl >/dev/null || die "curl not found — the watchdog cannot send without it"
command -v notify-send >/dev/null || say "  ⚠ notify-send missing: OnFailure has nowhere to shout."
[ -r "$ENV_FILE" ] || die "no credentials at $ENV_FILE — run \`bash scripts/setup-token.sh\` first"
grep -q '^HERDR_TG_TOKEN=' "$ENV_FILE" || die "$ENV_FILE has no HERDR_TG_TOKEN"
grep -q '^HERDR_TG_ALLOWED_CHAT_IDS=' "$ENV_FILE" \
  || die "$ENV_FILE has no HERDR_TG_ALLOWED_CHAT_IDS — the watchdog would have nobody to alarm"
perms=$(stat -c '%a' "$ENV_FILE")
case "$perms" in 600|400) ;; *) die "$ENV_FILE is mode $perms and holds a bot token. Fix: chmod 600 $ENV_FILE" ;; esac

# Without linger a --user timer stops at logout and never returns at boot. A watchdog that is off
# is worse than none, because you believe you have one.
if [ "$(loginctl show-user "$USER" --property=Linger --value 2>/dev/null || echo no)" != "yes" ]; then
  say "  ⚠ linger is OFF for $USER: this timer stops at logout and does not start at boot."
  say "    Enable it with:  sudo loginctl enable-linger $USER"
fi

mkdir -p "$HOME/.local/bin" "$UNIT_DIR" "$STATE_DIR"
install -m 0755 "$REPO/deploy/herdr-tg-watchdog.sh" "$BIN_DST"
install -m 0644 "$REPO/deploy/herdr-tg-watchdog.service"        "$UNIT_DIR/herdr-tg-watchdog.service"
install -m 0644 "$REPO/deploy/herdr-tg-watchdog-failed.service" "$UNIT_DIR/herdr-tg-watchdog-failed.service"
install -m 0644 "$REPO/deploy/herdr-tg-watchdog.timer"          "$UNIT_DIR/herdr-tg-watchdog.timer"
systemctl --user daemon-reload
say "  installed the script, both units and the timer"

# ── proof 1: the decision, with nothing sent ─────────────────────────────────
say
say "Proof 1 — a hub that stopped answering an hour ago really does produce an alarm…"
STAGE="$(mktemp -d)"; trap 'rm -rf "$STAGE"' EXIT
: > "$STAGE/hub.heartbeat"; touch -d '1 hour ago' "$STAGE/hub.heartbeat"
: > "$STAGE/watchdog.armed"
# A watchdog that has just started, or that woke from a suspend, gives the hub one full window
# before judging it. Stage a previous check a minute ago so this proves the STEADY-STATE decision
# and not the grace window.
printf '%s 0\n' "$(( $(date +%s) - 60 ))" > "$STAGE/watchdog.tick"
DRY="$(HERDR_TG_STATE_DIR="$STAGE" HERDR_TG_ENV_FILE=/dev/null "$BIN_DST" --dry-run 2>/dev/null)" \
  || die "the watchdog exited non-zero deciding on a stale hub"
printf '%s\n' "$DRY" | sed 's/^/  │ /'
printf '%s' "$DRY" | grep -q 'phone line is down' || die "a stale hub produced no alarm text"
# The alarm must name the disarm file of the state dir it is actually watching — the failure this
# replaced was an alarm that named a temp directory deleted seconds later. Checked twice: the text
# names the dir it was given, and the INSTALLED unit is pointed at the operator's real one.
printf '%s' "$DRY" | grep -qF "$STAGE/watchdog.disarmed" \
  || die "the alarm does not name the disarm file of the directory it is watching"
grep -qF "Environment=HERDR_TG_STATE_DIR=%h/.local/state/herdr-tg" "$UNIT_DIR/herdr-tg-watchdog.service" \
  || die "the installed unit does not point at $STATE_DIR"

# A hub that has never run must stay completely silent. This is the state the box is in today, and
# getting it wrong means buzzing the operator about something that was never built.
SILENT="$(mktemp -d)"
out="$(HERDR_TG_STATE_DIR="$SILENT" HERDR_TG_ENV_FILE=/dev/null "$BIN_DST" --dry-run 2>&1)"; rc=$?
rm -rf "$SILENT"
[ "$rc" -eq 0 ] && [ -z "$out" ] || die "a never-armed watchdog was not silent (rc=$rc): $out"
say "  ✓ stale hub alarms; never-armed hub is silent"

if [ "$PROVE" != yes ]; then
  systemctl --user enable herdr-tg-watchdog.timer >/dev/null
  say; say "--dry: nothing was sent. The send path is UNPROVEN."; exit 0
fi

# ── proof 2: the send, through the installed unit's own sandbox ──────────────
# The -p list is read out of the unit file rather than retyped, so this cannot drift away from
# what the timer actually runs.
say
say "Proof 2 — sending one drill message through the unit's own sandbox…"
# `%h` is a unit-file specifier that systemd expands and `systemd-run -p` does not, so a literal
# `%h/.config/herdr-tg/env` would make the drill fail for a reason that has nothing to do with the
# alarm. Expand it here, the same way systemd would.
mapfile -t PROPS < <(awk '/^\[Service\]/{s=1;next} /^\[/{s=0} s && /^[A-Za-z]+=/ && !/^ExecStart=/ && !/^Type=/ {print "-p"; print $0}' \
                     "$UNIT_DIR/herdr-tg-watchdog.service" | sed "s#%h#$HOME#g")
systemd-run --user --wait --collect --quiet --pty "${PROPS[@]}" "$BIN_DST" --test \
  || die "the drill did not send. The watchdog is installed and CANNOT reach you."

# ── proof 3, on request: the whole path, decision and send together ─────────
# --test proves the send and --dry-run proves the decision, but nothing proves the ten lines
# between them. This does, at the cost of one message that reads like a genuine outage — so it is
# opt-in, and you should be looking at your phone when you run it.
#
# The staging directory goes under $HOME, NOT under /tmp. The unit sets PrivateTmp=yes, so a
# staged state dir in /tmp is invisible to it: the watchdog sees an empty directory, decides there
# is nothing to watch, and exits 0. That looks exactly like a pass and proves nothing.
if [ "$FIRE" = yes ]; then
  say
  say "Proof 3 — firing one REAL alarm for a hub staged as dead three hours ago…"
  DRILL="$HOME/.local/state/herdr-tg-drill"; rm -rf "$DRILL"; mkdir -p "$DRILL"
  : > "$DRILL/hub.heartbeat"; touch -d '3 hours ago' "$DRILL/hub.heartbeat"
  : > "$DRILL/watchdog.armed"; printf '%s 0\n' "$(( $(date +%s) - 60 ))" > "$DRILL/watchdog.tick"
  systemd-run --user --wait --collect --quiet --pty "${PROPS[@]}" \
    -p "Environment=HERDR_TG_STATE_DIR=$DRILL" "$BIN_DST" \
    || { rm -rf "$DRILL"; die "the full path did not deliver"; }
  [ "$(cat "$DRILL/watchdog.latch" 2>/dev/null)" = "1" ] \
    || { rm -rf "$DRILL"; die "it sent, but wrote no latch — the repeat interval would not work"; }
  rm -rf "$DRILL"
  say "  ✓ decision and send, end to end, through the unit's sandbox"
fi

systemctl --user enable --now herdr-tg-watchdog.timer >/dev/null
say "  timer enabled and started"
systemctl --user list-timers herdr-tg-watchdog.timer --no-pager | sed 's/^/  /'

say
say "✅ Telegram accepted the drill. Check your phone — HTTP 200 means the Bot API took it,"
say "   not that it reached you. Only you can confirm the buzz."
say
say "  checks:     every 60s; alarms after 180s of silence, then every ~30 min"
say "  arms:       the first time the hub ever stamps ~/.local/state/herdr-tg/hub.heartbeat"
say "  silence:    touch $STATE_DIR/watchdog.disarmed   (wears off after a day)"
say "  if it dies: the unit goes 'failed' and notify-send shouts at the screen"
say "  logs:       journalctl --user -u herdr-tg-watchdog -f"
say "  remove:     systemctl --user disable --now herdr-tg-watchdog.timer"
