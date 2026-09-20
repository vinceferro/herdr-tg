#!/usr/bin/env bash
# Install Kickoff Channel as a systemd --user service, and PROVE it came up.
#
# The point of this script is the last step. Installing a unit is easy; the failure this guards
# against is a unit that installs cleanly, starts, and then sits dead in a restart loop while the
# operator — who is on a phone — sees only a bot that never answers. So it tails the journal until
# the bridge logs that it is long-polling, and fails loudly if it does not.
#
#     bash scripts/install-service.sh
#
# Idempotent: re-running rebuilds, reinstalls and restarts.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
UNIT="kickoff-channel.service"
UNIT_DST="$UNIT_DIR/$UNIT"
# The app plane's unit, laid down here but NOT started — see the install step for why both files and
# only one enable.
APP_UNIT="kickoff-channel-app.service"
# The units these replace. A box that ran the former names keeps its copies for ever otherwise, and
# they are not harmless together: the old phone unit names the OLD app unit in `Conflicts=` and the
# new one names the new, so systemd would happily run one plane of each — both binding the same
# socket and fighting over the same single-hub lock, which is the exact thing `Conflicts=` exists to
# make impossible. Both former names are retired because this script now lays down both planes.
SUPERSEDED_UNITS=(herdr-tg.service herdr-tg-app.service)
# The config home is NOT part of this rename, and the unit files still name this exact path. Moving
# it would strand the credential of every box that already has one.
ENV_FILE="$HOME/.config/herdr-tg/env"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"

# Every command this repo ships, and BOTH directories that can answer for them.
#
# `~/.cargo/bin` shadows `~/.local/bin` on this box's PATH, and installing only the latter has bitten
# twice: the shell goes on running a build from before the change while everything you just installed
# sits one directory further down. `kickoff-channel` is the product's name and `herdr-tg` is the same
# program under the name three other organisations already call LIVE, so both are laid down; neither
# is a copy of a stale build, because both come out of the release build below.
BIN_DIRS=("$HOME/.local/bin" "$HOME/.cargo/bin")
COMMANDS=(kickoff-channel herdr-tg kickoff-door)

say()  { printf '%s\n' "$*"; }
die()  { printf '\nerror: %s\n' "$*" >&2; exit 1; }

say "Kickoff Channel — install as a systemd --user service"
say "──────────────────────────────────────────────────────────────────"

# ── preflight ────────────────────────────────────────────────────────────────
for src in "$REPO/deploy/$UNIT" "$REPO/deploy/$APP_UNIT"; do
  [ -f "$src" ] || die "unit file missing: $src"
done
command -v systemctl >/dev/null || die "systemctl not found — this needs systemd"
[ -x "$CARGO" ] || die "cargo not found at $CARGO (set \$CARGO to override)"

if [ ! -f "$ENV_FILE" ]; then
  die "no credentials at $ENV_FILE — run \`bash scripts/setup-token.sh\` first"
fi
# Never print the token; assert only that the key is present and the file is not world-readable.
# Either spelling counts. The binary reads the current name and the former one, refusing only if
# both are set to different bytes, and an installer stricter than the program it installs would
# refuse a perfectly good credential file for the name somebody wrote on it.
env_key() {   # $1 suffix -> prints the spelling actually present, or nothing
  local suffix="$1"
  if   grep -q "^KICKOFF_CHANNEL_${suffix}=" "$ENV_FILE"; then printf 'KICKOFF_CHANNEL_%s' "$suffix"
  elif grep -q "^HERDR_TG_${suffix}="        "$ENV_FILE"; then printf 'HERDR_TG_%s' "$suffix"
  fi
}
TOKEN_KEY="$(env_key TOKEN)"
[ -n "$TOKEN_KEY" ] \
  || die "$ENV_FILE has no KICKOFF_CHANNEL_TOKEN (or HERDR_TG_TOKEN) — re-run setup-token.sh"
perms=$(stat -c '%a' "$ENV_FILE")
case "$perms" in
  600|400) ;;
  *) die "$ENV_FILE is mode $perms — it holds a bot token. Fix: chmod 600 $ENV_FILE" ;;
esac
CHATS_KEY="$(env_key ALLOWED_CHAT_IDS)"
USERS_KEY="$(env_key ALLOWED_USER_IDS)"
if [ -z "$CHATS_KEY" ]; then
  say "  ⚠ no chat allowlist — the bot will start but answer NOBODY."
  say "    That is the fail-closed default, not a bug. Re-run setup-token.sh to add your chat id."
else
  # A private chat's id is its person's id, so every POSITIVE id on the chat allowlist is a person
  # who may type at every agent and tap every button — the operator's own chat is the one this is
  # for. Said here, once per chat, because a teammate's chat added so /projects works for him in
  # private is the same grant, and nothing else at install time would say so. Only the chat-id
  # line is read; the token line is never touched. The variable is quoted back in the spelling his
  # own file uses, because a sentence naming a name he never wrote reads as a bug rather than a fix.
  private_chats=$(sed -n "s/^${CHATS_KEY}=//p" "$ENV_FILE" | tr -d "\"'" | tr -d '[:space:]' | tr ',' '\n' | grep '^[0-9]' || true)
  for id in $private_chats; do
    say "  ℹ chat $id is a private chat, so that person may type at EVERY agent and tap EVERY button."
    say "    For one project only: take it off $CHATS_KEY and run  kickoff-channel allow <repo> $id"
  done
  if [ -z "$USERS_KEY" ] && [ -z "$private_chats" ]; then
    say "  ⚠ no private chat on $CHATS_KEY and no user allowlist — NOBODY may speak."
    say "    Add your own private chat to the chat allowlist, or list your user id in KICKOFF_CHANNEL_ALLOWED_USER_IDS."
  fi
fi

# `Linger` is what lets a --user service run without an active login session. Without it the bridge
# dies at logout and never comes back at boot — the failure looks identical to a crash.
if [ "$(loginctl show-user "$USER" --property=Linger --value 2>/dev/null || echo no)" != "yes" ]; then
  say "  ⚠ linger is OFF for $USER: the service will stop at logout and not start at boot."
  say "    Enable it with:  sudo loginctl enable-linger $USER"
fi

# ── build ────────────────────────────────────────────────────────────────────
say
say "Building --release (the service must not run a debug binary)…"
( cd "$REPO" && env -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" "$CARGO" build --release ) \
  || die "release build failed"

say
for cmd in "${COMMANDS[@]}"; do
  [ -x "$REPO/target/release/$cmd" ] \
    || die "the release build produced no $cmd — the unit would start a program that is not there"
done
for BIN_DIR in "${BIN_DIRS[@]}"; do
  mkdir -p "$BIN_DIR"
  # Every destination spelled out rather than composed from a loop variable, and that is not style:
  # `tests/every_unit_starts_a_command_an_installer_here_really_lays_down.rs` reads this file to
  # check that every `ExecStart=%h/.local/bin/<name>` under deploy/ is a name some installer really
  # writes, and a destination built out of a variable is one that guard cannot see. A unit starting
  # a binary nobody installs does not fail loudly — under Restart=always it fails every five
  # seconds, for ever, on a box he only reaches from his phone.
  install -m 0755 "$REPO/target/release/kickoff-channel" "$BIN_DIR/kickoff-channel"
  install -m 0755 "$REPO/target/release/herdr-tg"        "$BIN_DIR/herdr-tg"
  install -m 0755 "$REPO/target/release/kickoff-door"    "$BIN_DIR/kickoff-door"
done
# Report the copy the SHELL finds, not the one we just wrote. That is the whole point of installing
# two directories, and printing our own path back would hide exactly the shadowing this guards.
hash -r 2>/dev/null || true
for cmd in "${COMMANDS[@]}"; do
  found="$(command -v "$cmd" || true)"
  say "  installed $cmd into ${BIN_DIRS[*]} — your shell finds ${found:-nothing}"
done
say "  version:  $(kickoff-channel --version 2>/dev/null || "$REPO/target/release/kickoff-channel" --version)"

# ── install the units ────────────────────────────────────────────────────────
# BOTH hub planes' unit files go down here, and exactly one of them is enabled.
#
# Laying a file down and enabling it are different acts, and only the first is about names. The one
# remedy this product prints for the likeliest way this box loses its taps — a second copy of the
# hub holding the line — names BOTH planes, because the copy already holding the lock may be either
# and the lock file carries a pid and nothing else. A name he pastes into `systemctl` that is not on
# his box answers "Unit not found" and leaves him at a keyboard with nothing left to try, so the
# remedy is only true if something put both files there. Nothing did: until now the app plane's unit
# existed in this repo and in a runbook, which is another way of saying nobody installed it.
#
# Enabling both would be worse than enabling neither. Both planes bind the same socket and take the
# same single-hub lock, and each names the other in `Conflicts=`, so starting the app plane here
# would stop the phone plane this whole script exists to prove came up. Which plane a box runs is
# his decision, it is one line, and the parting hints below say what that line is.
#
# Destinations are spelled out rather than composed from a variable, for the same reason as the
# binaries above: the guard
# `tests/every_unit_this_product_names_in_an_instruction_is_one_an_installer_here_lays_down.rs`
# reads this file to check that every unit this product tells him to act on is one some installer
# here really writes into his unit directory, and a destination built out of a variable is one that
# guard cannot see.
mkdir -p "$UNIT_DIR"
install -m 0644 "$REPO/deploy/kickoff-channel.service"     "$UNIT_DIR/kickoff-channel.service"
install -m 0644 "$REPO/deploy/kickoff-channel-app.service" "$UNIT_DIR/kickoff-channel-app.service"
say "  installed $UNIT_DST"
say "  installed $UNIT_DIR/$APP_UNIT — the app plane, in place but NOT started"

# Retire the names these units replace, and ONLY where the file on disk is ours. A user manager's
# unit names are one namespace shared with every other organisation on this box, so a blind
# `disable --now` on a name we no longer ship is how one project stops another project's service.
for superseded in "${SUPERSEDED_UNITS[@]}"; do
  OLD="$UNIT_DIR/$superseded"
  [ -f "$OLD" ] || continue
  if grep -q 'github.com/vinceferro/herdr-tg' "$OLD"; then
    systemctl --user disable --now "$superseded" >/dev/null 2>&1 || true
    rm -f "$OLD"
    say "  retired $superseded — it is one of these units under its former name"
  else
    say "  ⚠ $OLD exists and is NOT ours; left alone. Two hubs may now fight over one socket."
  fi
done

systemctl --user daemon-reload
systemctl --user enable "$UNIT" >/dev/null
say "  enabled at boot"

# Capture the journal cursor BEFORE restarting, so the check reads only this run's lines and cannot
# pass on a previous successful start.
CURSOR=$(journalctl --user -u "$UNIT" -n0 --show-cursor 2>/dev/null \
         | sed -n 's/^-- cursor: //p' || true)

systemctl --user restart "$UNIT"
say "  started"

# ── prove it came up ─────────────────────────────────────────────────────────
say
say "Waiting for the bridge to reach the Bot API…"
DEADLINE=$((SECONDS + 45))
ok=""
while [ $SECONDS -lt $DEADLINE ]; do
  if [ -n "$CURSOR" ]; then
    LOGS=$(journalctl --user -u "$UNIT" --after-cursor "$CURSOR" --no-pager 2>/dev/null || true)
  else
    LOGS=$(journalctl --user -u "$UNIT" -n 200 --no-pager 2>/dev/null || true)
  fi
  if printf '%s' "$LOGS" | grep -q 'long-polling'; then ok=yes; break; fi
  # Fail fast on the errors that will never resolve by waiting. The token sentence is matched on
  # its suffix, because the binary names the variable in whichever spelling it looked for, and a
  # pattern pinned to one of them stops firing the day the other is the one it prints.
  if printf '%s' "$LOGS" | grep -qi 'TOKEN is not set'; then
    die "the service started but has no token — run scripts/setup-token.sh"
  fi
  if printf '%s' "$LOGS" | grep -qi 'herdr unreachable'; then
    die "the service cannot reach herdr's socket. Is herdr running? \`herdr status\`"
  fi
  sleep 1
done

say
if [ -n "$ok" ]; then
  systemctl --user --no-pager --lines=0 status "$UNIT" | sed 's/^/  /' || true
  say
  say "✅ Kickoff Channel is live and long-polling."
  printf '%s' "$LOGS" | grep -E 'allowlist|long-polling' | sed 's/^/  /' || true
  say
  say "Send /status to your bot. It will be there after a reboot, after a crash, and after"
  say "this session ends."
  say
  say "  logs:     journalctl --user -u kickoff-channel -f"
  say "  restart:  systemctl --user restart kickoff-channel"
  say "  stop:     systemctl --user stop kickoff-channel"
  say "  remove:   systemctl --user disable --now kickoff-channel && rm $UNIT_DST"
  say
  # Said here because this is the only place he learns the app plane's file is now on his box. It
  # is deliberately one line and deliberately his: the two planes stop each other, so switching is
  # something he does knowing which one he wants, not something an installer decides for him.
  say "The app plane is installed too, and stopped. It holds no token and dials nothing off this"
  say "box; it reaches you through the kickoff app instead. Switching is one line, and it stops"
  say "this one:"
  say "  systemctl --user enable --now kickoff-channel-app"
else
  say "❌ the service did not reach the Bot API within 45s."
  say
  systemctl --user --no-pager --lines=20 status "$UNIT" | sed 's/^/  /' || true
  say
  say "Recent log:"
  printf '%s\n' "$LOGS" | tail -20 | sed 's/^/  /'
  die "not healthy — do not assume it will recover on its own"
fi
