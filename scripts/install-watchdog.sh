#!/usr/bin/env bash
# Install the Kickoff Channel watchdog — and PROVE it, so it is not a watchdog you are guessing about.
#
#     bash scripts/install-watchdog.sh          # install, prove the decision, prove the send
#     bash scripts/install-watchdog.sh --dry    # install only; send nothing
#     bash scripts/install-watchdog.sh --fire   # also send ONE real outage alarm for a staged hub
#     bash scripts/install-watchdog.sh --prove  # prove the decision only; install NOTHING
#
# `--dry` is not a dry run of the install: it lays the script, both units and the timer down and
# enables the timer, and only the SEND is skipped. `--prove` is the one that touches nothing — it
# runs proof 1 against this repo's own copy of the script and exits. Somebody reading this file to
# find a safe way to try it wanted that one and could not find it, because it did not exist.
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
# The name deploy/kickoff-channel-watchdog.service starts. A unit pointing at a binary no installer
# writes is a unit that fails at start every five seconds, so these two move together or not at all.
BIN_DST="$HOME/.local/bin/kickoff-channel-watchdog"
ENV_FILE="$HOME/.config/herdr-tg/env"
STATE_DIR="$HOME/.local/state/herdr-tg"
PROVE=yes; FIRE=no; INSTALL=yes
case "${1:-}" in
  --dry)   PROVE=no ;;
  --fire)  FIRE=yes ;;
  # Proves and installs nothing. The decision is the half that can be proved without laying a
  # single file down, and without it the only way to try this script was to install it.
  --prove) INSTALL=no ;;
  '')      ;;
  *)       printf 'usage: install-watchdog.sh [--dry|--fire|--prove]\n' >&2; exit 2 ;;
esac

say() { printf '%s\n' "$*"; }
die() { printf '\nerror: %s\n' "$*" >&2; exit 1; }

# Take down the timer and units this install replaces — and ONLY where the file on disk is ours.
#
# A user manager has one unit namespace for every organisation on the box, so a blind
# `disable --now` on a name we no longer ship is how one project stops another project's service.
# Leaving them is not an option either: the old timer runs its own copy of the script every minute
# beside the new one, so he gets every alarm twice and every drill twice, which teaches him to
# ignore the one message that has to be trusted.
retire_superseded() {
  local unit
  for unit in herdr-tg-watchdog.timer herdr-tg-watchdog.service herdr-tg-watchdog-failed.service; do
    [ -f "$UNIT_DIR/$unit" ] || continue
    if grep -q 'github.com/vinceferro/herdr-tg' "$UNIT_DIR/$unit"; then
      systemctl --user disable --now "$unit" >/dev/null 2>&1 || true
      rm -f "$UNIT_DIR/$unit"
      say "  retired $unit — it is this watchdog under its former name"
    else
      say "  ⚠ $UNIT_DIR/$unit exists and is NOT ours; left alone. You may now be alarmed twice."
    fi
  done
}

# An agent session on this box inherits TMPDIR as the literal string "%h/.cache/tmp" — an
# unexpanded systemd specifier — so mktemp hands back a RELATIVE path. CLAUDE.md documents this for
# cargo; it bites shell just as hard, and here it made the staged proof print a nonsense path.
case "${TMPDIR:-/tmp}" in
  /*) ;;
  *)  TMPDIR=/tmp; export TMPDIR ;;
esac

say "Kickoff Channel watchdog — install and prove"
say "──────────────────────────────────────────────────────────────────"

for f in deploy/herdr-tg-watchdog.sh deploy/kickoff-channel-watchdog.service \
         deploy/kickoff-channel-watchdog-failed.service deploy/kickoff-channel-watchdog.timer; do
  [ -f "$REPO/$f" ] || die "missing $f"
done
command -v systemctl >/dev/null || die "systemctl not found — this needs systemd"
command -v curl >/dev/null || die "curl not found — the watchdog cannot send without it"
command -v notify-send >/dev/null || say "  ⚠ notify-send missing: OnFailure has nowhere to shout."

# The credential is what the SEND needs, and --prove does not send. Demanding a bot token before
# proving a decision would put the one safe mode behind the one thing an app-plane box does not
# have — and this box is on the app plane.
if [ "$INSTALL" = yes ]; then
  [ -r "$ENV_FILE" ] || die "no credentials at $ENV_FILE — run \`bash scripts/setup-token.sh\` first"
  # Either spelling counts. The hub reads the current name and the former one, and an installer
  # stricter than the program it installs refuses a perfectly good credential file over the name
  # somebody wrote on it.
  grep -qE '^(KICKOFF_CHANNEL|HERDR_TG)_TOKEN=' "$ENV_FILE" \
    || die "$ENV_FILE has no KICKOFF_CHANNEL_TOKEN (or HERDR_TG_TOKEN)"
  grep -qE '^(KICKOFF_CHANNEL|HERDR_TG)_ALLOWED_CHAT_IDS=' "$ENV_FILE" \
    || die "$ENV_FILE has no chat allowlist — the watchdog would have nobody to alarm"
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
  install -m 0644 "$REPO/deploy/kickoff-channel-watchdog.service"        "$UNIT_DIR/kickoff-channel-watchdog.service"
  install -m 0644 "$REPO/deploy/kickoff-channel-watchdog-failed.service" "$UNIT_DIR/kickoff-channel-watchdog-failed.service"
  install -m 0644 "$REPO/deploy/kickoff-channel-watchdog.timer"          "$UNIT_DIR/kickoff-channel-watchdog.timer"
  retire_superseded
  systemctl --user daemon-reload
  say "  installed the script, both units and the timer"
fi

# What proof 1 runs, and which unit it holds the state directory against. Installed, they are the
# copies the timer will really use; under --prove they are this repo's, which is what the install
# would have copied — said out loud below rather than left for the reader to work out.
WD="$BIN_DST"; UNIT_UNDER_PROOF="$UNIT_DIR/kickoff-channel-watchdog.service"
if [ "$INSTALL" != yes ]; then
  WD="$REPO/deploy/herdr-tg-watchdog.sh"
  UNIT_UNDER_PROOF="$REPO/deploy/kickoff-channel-watchdog.service"
  say "  --prove: nothing installed, nothing enabled. Proving this repo's copy."
fi

# ── proof 1: the decision, with nothing sent ─────────────────────────────────
say
say "Proof 1 — a hub that stopped answering an hour ago really does produce an alarm…"
STAGE="$(mktemp -d)"; trap 'rm -rf "$STAGE"' EXIT
# Staged the way the hub really writes it — a word in the stamp and a four-line note beside it, one
# sentence per leg — so this proves the decision the operator will actually get and not a shape only
# a fixture makes.
stage_hub() {   # $1 dir, $2 stamp age, $3 note age, $4 phone, $5 door, $6 update line
  printf 'serving\n' > "$1/hub.heartbeat"; touch -d "$2" "$1/hub.heartbeat"
  printf 'not serving\n%s\n%s\n%s\n' "$4" "$5" "$6" > "$1/hub.health"; touch -d "$3" "$1/hub.health"
  : > "$1/watchdog.armed"
  printf '%s 0\n' "$(( $(date +%s) - 60 ))" > "$1/watchdog.tick"
}
PHONE_OK="the phone line answered 12 seconds ago"
DOOR_OK="the agents' door let a connection through 12 seconds ago"
UPD_OK="the hub looked for your taps 12 seconds ago"
stage_hub "$STAGE" '1 hour ago' 'now' "the phone line last answered 1 hour ago" "$DOOR_OK" "$UPD_OK"
# stage_hub also writes watchdog.tick a minute back: a watchdog that has just started, or that woke
# from a suspend, gives the hub one full window before judging it, and this must prove the
# STEADY-STATE decision rather than the grace window.
DRY="$(HERDR_TG_STATE_DIR="$STAGE" HERDR_TG_ENV_FILE=/dev/null "$WD" --dry-run 2>/dev/null)" \
  || die "the watchdog exited non-zero deciding on a stale hub"
printf '%s\n' "$DRY" | sed 's/^/  │ /'
printf '%s' "$DRY" | grep -q 'phone line is down' || die "a stale hub produced no alarm text"
# The alarm must name the disarm file of the state dir it is actually watching — the failure this
# replaced was an alarm that named a temp directory deleted seconds later. Checked twice: the text
# names the dir it was given, and the INSTALLED unit is pointed at the operator's real one.
printf '%s' "$DRY" | grep -qF "$STAGE/watchdog.disarmed" \
  || die "the alarm does not name the disarm file of the directory it is watching"
# The variable keeps its former spelling and so does the path, and both halves are deliberate. The
# only reader of this setting is deploy/herdr-tg-watchdog.sh — a shell script sharing no code with
# the hub — and the binary's two-name reader (`compat.rs`) never sees this suffix, so a unit saying
# `KICKOFF_CHANNEL_STATE_DIR` would be read by nobody and the script would fall back to its
# `${XDG_…:-…}` default: the one silent failure this whole file exists to prevent. The path is
# hard-coded by other organisations on this box and does not move either.
grep -qF "Environment=HERDR_TG_STATE_DIR=%h/.local/state/herdr-tg" "$UNIT_UNDER_PROOF" \
  || die "the installed unit does not point at $STATE_DIR"

# A hub that has never run must stay completely silent. This is the state the box is in today, and
# getting it wrong means buzzing the operator about something that was never built.
SILENT="$(mktemp -d)"
out="$(HERDR_TG_STATE_DIR="$SILENT" HERDR_TG_ENV_FILE=/dev/null "$WD" --dry-run 2>&1)"; rc=$?
rm -rf "$SILENT"
[ "$rc" -eq 0 ] && [ -z "$out" ] || die "a never-armed watchdog was not silent (rc=$rc): $out"

# The half with no symptom on the phone. The bot keeps answering Telegram, so every check he can
# make by hand says "fine", while no agent can reach him at all. If this is the proof that ever
# fails, the alarm has quietly gone back to watching one thing and naming it wrong.
OTHER="$(mktemp -d)"
stage_hub "$OTHER" '1 hour ago' 'now' "$PHONE_OK" \
  "the agents' door has let nothing through since this hub started" "$UPD_OK"
# `|| { … die … }`, because under `set -e` a bare assignment from a failing command substitution
# ends the script where it stands: no message, no cleanup, and the units already laid down. The
# operator would be left with a half-finished install and nothing said about why.
ODRY="$(HERDR_TG_STATE_DIR="$OTHER" HERDR_TG_ENV_FILE=/dev/null "$WD" --dry-run 2>/dev/null)" \
  || { rm -rf "$OTHER"; die "the watchdog exited non-zero deciding on a hub whose door had died"; }
rm -rf "$OTHER"
printf '%s' "$ODRY" | grep -q 'agents cannot reach it' \
  || die "an agents' door that stopped accepting produced no alarm naming it"
# `if`, not `&& die`: this script runs under `set -e`, and a `&&` chain whose test FAILS is the
# passing case here — written the other way it would take the whole install down on success.
if printf '%s' "$ODRY" | grep -q 'phone line is down'; then
  die "it blamed the phone line for an outage on the agents' side; he would go and fix the wrong thing"
fi

# The leg with no symptom ANYWHERE. Telegram answers the bot, agents get through the door, and
# Telegram has stopped handing this copy what the operator sends — a second copy of the bot is
# holding the long poll. Every check he can make by hand says fine and every tap he makes dies in
# silence, so if any proof here earns its minute, it is this one.
TAPS="$(mktemp -d)"
stage_hub "$TAPS" '1 hour ago' 'now' "$PHONE_OK" "$DOOR_OK" \
  "another copy of this bot is taking your taps, so none of them reach the agents here"
TDRY="$(HERDR_TG_STATE_DIR="$TAPS" HERDR_TG_ENV_FILE=/dev/null "$WD" --dry-run 2>/dev/null)" \
  || { rm -rf "$TAPS"; die "the watchdog exited non-zero deciding on a hub whose taps stopped arriving"; }
rm -rf "$TAPS"
printf '%s' "$TDRY" | grep -q 'nothing you tap is reaching an agent' \
  || die "an update line that stopped carrying his taps produced no alarm naming it"
printf '%s' "$TDRY" | grep -q 'second copy of this bot' \
  || die "the alarm named the leg but not the one thing he can check in ten seconds"
if printf '%s' "$TDRY" | grep -q 'phone line is down'; then
  die "it blamed the phone line for an outage on the update line; the phone in his hand is working"
fi
if printf '%s' "$TDRY" | grep -q 'agents cannot reach it'; then
  die "it blamed the agents' door for an outage on the update line; agents are getting in fine"
fi

# ── the other plane, all three of its legs ───────────────────────────────────
# A hub that reaches him through the app has no phone line and no stream of taps from a messaging
# app; its three legs are what agents say being written down for the app, the same door, and the
# sweep that collects his answers. A watchdog that did not know those words would report every
# outage on such a box as total and would word the fix for a bot that is not there.
#
# All three are driven, one at a time, because the line printed below says all three are. It used
# to drive exactly one — the sweep — and print the sentence anyway. That is the shape this repo has
# already been burnt by: a `cargo test` filter that matched nothing was a pass, it sat in the step
# deciding whether a bridge got installed, and an operator was told his plugin was unsafe when his
# plugin was fine. A gate that reports on ground it did not walk is worse than no gate, because it
# is believed. `tests/an_installer_ticks_only_the_legs_it_drove.rs` now fails the workspace if the
# tick outruns the staging again.
APP_RING_OK="what agents say is being written down for the app"
APP_RING_BAD="what agents say cannot be written down for the app to read"
APP_SWEEP_OK="the hub went to collect your answers 12 seconds ago"
APP_SWEEP_BAD="the place your answers arrive could not be read, so none of them are getting through"
# A different sentence for the door than the phone-plane case above uses. The hub has several ways
# of saying this leg is down and the watchdog classifies them all as one; driving a second spelling
# here costs nothing and means the two cases cannot both be passing on one arm of that classifier.
APP_DOOR_BAD="the agents' door could not be opened"

# Leg one: nothing an agent says is being written down, while the door and the sweep are fine. He
# is holding an app that has stopped filling and everything else about the box looks well.
APP="$(mktemp -d)"
stage_hub "$APP" '1 hour ago' 'now' "$APP_RING_BAD" "$DOOR_OK" "$APP_SWEEP_OK"
ARING="$(HERDR_TG_STATE_DIR="$APP" HERDR_TG_ENV_FILE=/dev/null "$WD" --dry-run 2>/dev/null)" \
  || { rm -rf "$APP"; die "the watchdog exited non-zero deciding on an app hub that stopped writing down what agents say"; }
rm -rf "$APP"
printf '%s' "$ARING" | grep -q 'nothing it says is reaching your app' \
  || die "an app hub that stopped writing down what agents say produced no alarm naming it"
if printf '%s' "$ARING" | grep -q 'nothing you tap is reaching an agent'; then
  die "it told him his taps were going nowhere while the hub says it collected some seconds ago"
fi
if printf '%s' "$ARING" | grep -q 'agents cannot reach'; then
  die "it blamed the agents' door, which the hub says is letting connections through"
fi

# Leg two: the door, on this plane. The same sentence on the phone line produces a different alarm
# beside different healthy legs, so proving it there proves nothing here.
APP="$(mktemp -d)"
stage_hub "$APP" '1 hour ago' 'now' "$APP_RING_OK" "$APP_DOOR_BAD" "$APP_SWEEP_OK"
ADOOR="$(HERDR_TG_STATE_DIR="$APP" HERDR_TG_ENV_FILE=/dev/null "$WD" --dry-run 2>/dev/null)" \
  || { rm -rf "$APP"; die "the watchdog exited non-zero deciding on an app hub whose door stopped opening"; }
rm -rf "$APP"
printf '%s' "$ADOOR" | grep -q 'agents cannot reach the herd' \
  || die "an app hub whose door stopped opening produced no alarm naming it"
printf '%s' "$ADOOR" | grep -qF "The hub says: $APP_DOOR_BAD" \
  || die "it named the leg but dropped the hub's own reason, which is the one thing he can act on"
if printf '%s' "$ADOOR" | grep -q 'nothing it says is reaching your app'; then
  die "it blamed the record the app reads, which the hub says it is still writing"
fi

# Leg three: his answers are not being collected. Agents are connecting and what they say is
# arriving; everything he sends goes nowhere, in silence.
APP="$(mktemp -d)"
stage_hub "$APP" '1 hour ago' 'now' "$APP_RING_OK" "$DOOR_OK" "$APP_SWEEP_BAD"
ADRY="$(HERDR_TG_STATE_DIR="$APP" HERDR_TG_ENV_FILE=/dev/null "$WD" --dry-run 2>/dev/null)" \
  || { rm -rf "$APP"; die "the watchdog exited non-zero deciding on an app hub whose answers stopped being collected"; }
rm -rf "$APP"
printf '%s' "$ADRY" | grep -q 'nothing you tap is reaching an agent' \
  || die "an app hub whose answers stopped being collected produced no alarm naming it"
if printf '%s' "$ADRY" | grep -q 'nothing it says is reaching your app'; then
  die "it blamed the record the app reads, which the hub says it is still writing"
fi
# One check all three share, because it is the mistake that costs him the most time: an operator on
# this plane has no bot at all, and a word about his phone line sends him looking for one.
for a in "$ARING" "$ADOOR" "$ADRY"; do
  if printf '%s' "$a" | grep -q 'phone line'; then
    die "it told an operator with no phone line about his phone line; he would go looking for a bot that is not running"
  fi
done
say "  ✓ any of the three legs going quiet alarms, the alarm names which, on either plane;"
say "    a never-armed hub is silent"

# --prove stops here, before anything is laid down, enabled or sent. Proofs 2 and 3 are built from
# the INSTALLED unit's own directives, so there is nothing honest for them to read in this mode.
if [ "$INSTALL" != yes ]; then
  say
  say "--prove: the decision is proved and NOTHING was installed, enabled or sent."
  say "         The send path is UNPROVEN — that needs a real install and a real token."
  exit 0
fi

if [ "$PROVE" != yes ]; then
  systemctl --user enable kickoff-channel-watchdog.timer >/dev/null
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
                     "$UNIT_DIR/kickoff-channel-watchdog.service" | sed "s#%h#$HOME#g")
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
  # Both halves dead, which is what a stopped hub really looks like: the drill should read like the
  # alarm he would actually get, not like a shape only this script can produce.
  stage_hub "$DRILL" '3 hours ago' '3 hours ago' \
    "the phone line last answered 3 hours ago" \
    "the agents' door last let a connection through 3 hours ago" \
    "the hub last looked for your taps 3 hours ago"
  systemd-run --user --wait --collect --quiet --pty "${PROPS[@]}" \
    -p "Environment=HERDR_TG_STATE_DIR=$DRILL" "$BIN_DST" \
    || { rm -rf "$DRILL"; die "the full path did not deliver"; }
  [ "$(cat "$DRILL/watchdog.latch" 2>/dev/null)" = "1" ] \
    || { rm -rf "$DRILL"; die "it sent, but wrote no latch — the repeat interval would not work"; }
  rm -rf "$DRILL"
  say "  ✓ decision and send, end to end, through the unit's sandbox"
fi

systemctl --user enable --now kickoff-channel-watchdog.timer >/dev/null
say "  timer enabled and started"
systemctl --user list-timers kickoff-channel-watchdog.timer --no-pager | sed 's/^/  /'

say
say "✅ Telegram accepted the drill. Check your phone — HTTP 200 means the Bot API took it,"
say "   not that it reached you. Only you can confirm the buzz."
say
say "  checks:     every 60s; alarms after 180s of silence, then every ~30 min"
say "  arms:       the first time it sees either of the hub's own files in"
say "              ~/.local/state/herdr-tg/ — hub.heartbeat or hub.health. Both, because a"
say "              hub whose door never opened never earns a stamp to arm on."
say "  watches:    the hub withholds that stamp when ANY of three legs stops, and says which in"
say "              hub.health beside it. On the phone plane the three are the line out to Telegram,"
say "              the line your taps come back down, and the door agents arrive at. On the app"
say "              plane there is no phone line: they are what agents say being written down for"
say "              the app, the same door, and the sweep that collects your answers. The alarm"
say "              names the leg, because the fix differs — a held update line is usually a second"
say "              copy of the bot, and on the app plane there is no bot to be a second copy of."
say "  silence:    touch $STATE_DIR/watchdog.disarmed   (wears off after a day)"
say "  if it dies: the unit goes 'failed' and notify-send shouts at the screen"
say "  logs:       journalctl --user -u kickoff-channel-watchdog -f"
say "  remove:     systemctl --user disable --now kickoff-channel-watchdog.timer"
