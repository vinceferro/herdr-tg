#!/usr/bin/env bash
# Install herdr-tg as a systemd --user service, and PROVE it came up.
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
UNIT_SRC="$REPO/deploy/herdr-tg.service"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
UNIT_DST="$UNIT_DIR/herdr-tg.service"
BIN_DIR="$HOME/.local/bin"
BIN_DST="$BIN_DIR/herdr-tg"
ENV_FILE="$HOME/.config/herdr-tg/env"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"

say()  { printf '%s\n' "$*"; }
die()  { printf '\nerror: %s\n' "$*" >&2; exit 1; }

say "herdr-tg — install as a systemd --user service"
say "──────────────────────────────────────────────────────────────────"

# ── preflight ────────────────────────────────────────────────────────────────
[ -f "$UNIT_SRC" ] || die "unit file missing: $UNIT_SRC"
command -v systemctl >/dev/null || die "systemctl not found — this needs systemd"
[ -x "$CARGO" ] || die "cargo not found at $CARGO (set \$CARGO to override)"

if [ ! -f "$ENV_FILE" ]; then
  die "no credentials at $ENV_FILE — run \`bash scripts/setup-token.sh\` first"
fi
# Never print the token; assert only that the key is present and the file is not world-readable.
grep -q '^HERDR_TG_TOKEN=' "$ENV_FILE" || die "$ENV_FILE has no HERDR_TG_TOKEN — re-run setup-token.sh"
perms=$(stat -c '%a' "$ENV_FILE")
case "$perms" in
  600|400) ;;
  *) die "$ENV_FILE is mode $perms — it holds a bot token. Fix: chmod 600 $ENV_FILE" ;;
esac
if ! grep -q '^HERDR_TG_ALLOWED_CHAT_IDS=' "$ENV_FILE"; then
  say "  ⚠ no HERDR_TG_ALLOWED_CHAT_IDS — the bot will start but answer NOBODY."
  say "    That is the fail-closed default, not a bug. Re-run setup-token.sh to add your chat id."
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

mkdir -p "$BIN_DIR"
install -m 0755 "$REPO/target/release/herdr-tg" "$BIN_DST"
say "  installed $BIN_DST ($("$BIN_DST" --version))"

# ── install the unit ─────────────────────────────────────────────────────────
mkdir -p "$UNIT_DIR"
install -m 0644 "$UNIT_SRC" "$UNIT_DST"
say "  installed $UNIT_DST"

systemctl --user daemon-reload
systemctl --user enable herdr-tg.service >/dev/null
say "  enabled at boot"

# Capture the journal cursor BEFORE restarting, so the check reads only this run's lines and cannot
# pass on a previous successful start.
CURSOR=$(journalctl --user -u herdr-tg.service -n0 --show-cursor 2>/dev/null \
         | sed -n 's/^-- cursor: //p' || true)

systemctl --user restart herdr-tg.service
say "  started"

# ── prove it came up ─────────────────────────────────────────────────────────
say
say "Waiting for the bridge to reach the Bot API…"
DEADLINE=$((SECONDS + 45))
ok=""
while [ $SECONDS -lt $DEADLINE ]; do
  if [ -n "$CURSOR" ]; then
    LOGS=$(journalctl --user -u herdr-tg.service --after-cursor "$CURSOR" --no-pager 2>/dev/null || true)
  else
    LOGS=$(journalctl --user -u herdr-tg.service -n 200 --no-pager 2>/dev/null || true)
  fi
  if printf '%s' "$LOGS" | grep -q 'long-polling'; then ok=yes; break; fi
  # Fail fast on the errors that will never resolve by waiting.
  if printf '%s' "$LOGS" | grep -qi 'HERDR_TG_TOKEN is not set'; then
    die "the service started but has no token — run scripts/setup-token.sh"
  fi
  if printf '%s' "$LOGS" | grep -qi 'herdr unreachable'; then
    die "the service cannot reach herdr's socket. Is herdr running? \`herdr status\`"
  fi
  sleep 1
done

say
if [ -n "$ok" ]; then
  systemctl --user --no-pager --lines=0 status herdr-tg.service | sed 's/^/  /' || true
  say
  say "✅ herdr-tg is live and long-polling."
  printf '%s' "$LOGS" | grep -E 'allowlist|long-polling' | sed 's/^/  /' || true
  say
  say "Send /status to your bot. It will be there after a reboot, after a crash, and after"
  say "this session ends."
  say
  say "  logs:     journalctl --user -u herdr-tg -f"
  say "  restart:  systemctl --user restart herdr-tg"
  say "  stop:     systemctl --user stop herdr-tg"
  say "  remove:   systemctl --user disable --now herdr-tg && rm $UNIT_DST"
else
  say "❌ the service did not reach the Bot API within 45s."
  say
  systemctl --user --no-pager --lines=20 status herdr-tg.service | sed 's/^/  /' || true
  say
  say "Recent log:"
  printf '%s\n' "$LOGS" | tail -20 | sed 's/^/  /'
  die "not healthy — do not assume it will recover on its own"
fi
