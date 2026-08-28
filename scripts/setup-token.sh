#!/usr/bin/env bash
# Capture the herdr-tg bot credentials into the file the systemd unit will read.
#
# WHY THIS EXISTS: a bot token must not reach an agent transcript, a shell history file, or a
# process argument list. This script reads it from a hidden prompt (never argv), writes it to a
# 0600 file outside the repo, and never prints it back. The only place it is ever sent is
# api.telegram.org, which is the service it authenticates to.
#
# Run it yourself, in your own terminal:
#     bash scripts/setup-token.sh
#
# It is idempotent: re-running replaces the file and keeps a timestamped backup.

set -euo pipefail

ENV_DIR="${HERDR_TG_CONFIG_DIR:-$HOME/.config/herdr-tg}"
ENV_FILE="$ENV_DIR/env"
API="https://api.telegram.org"

die() { printf '\nerror: %s\n' "$*" >&2; exit 1; }
say() { printf '%s\n' "$*"; }

# curl with the URL on stdin, never in argv — a token in a command line is visible to every
# process on the box via /proc, and to `ps` in a screenshot.
api() {
  local method=$1 token=$2
  shift 2
  printf 'url = "%s/bot%s/%s"\n' "$API" "$token" "$method" |
    curl --silent --show-error --max-time 20 --config - "$@"
}

say "herdr-tg — bot credential setup"
say "──────────────────────────────────────────────────────────────────"
say "Target: $ENV_FILE (mode 0600, outside the git repo)"
say

# ── 1. the token ──────────────────────────────────────────────────────────────
say "Paste the token BotFather gave you. It will not be echoed."
printf 'token: '
IFS= read -rs TOKEN || true
printf '\n'
[ -n "${TOKEN:-}" ] || die "no token entered"

# BotFather's shape is <bot_id>:<35 chars of [A-Za-z0-9_-]>. Checking it here turns a silent
# 401-loop at 2am into an error now.
if ! printf '%s' "$TOKEN" | grep -Eq '^[0-9]{6,}:[A-Za-z0-9_-]{30,}$'; then
  say
  say "That does not look like a BotFather token (expected <digits>:<35+ chars>)."
  printf 'Continue anyway? [y/N] '
  read -r ans
  case "$ans" in [Yy]*) ;; *) die "aborted — nothing was written" ;; esac
fi

# ── 2. verify it against Telegram ─────────────────────────────────────────────
BOT_USERNAME=""
say
printf 'Verify the token with Telegram now? This sends it to %s, which is what it is for. [Y/n] ' "$API"
read -r ans
case "$ans" in
  [Nn]*) say "  skipped — the token is unverified" ;;
  *)
    resp=$(api getMe "$TOKEN") || die "could not reach $API (offline? proxy?)"
    if ! printf '%s' "$resp" | grep -q '"ok":true'; then
      # The response can echo the token in an error; print only the description field.
      desc=$(printf '%s' "$resp" | sed -n 's/.*"description":"\([^"]*\)".*/\1/p')
      die "Telegram rejected the token: ${desc:-unknown error}"
    fi
    BOT_USERNAME=$(printf '%s' "$resp" | sed -n 's/.*"username":"\([^"]*\)".*/\1/p')
    say "  ✅ verified — the bot is @${BOT_USERNAME:-unknown}"
    ;;
esac

# ── 3. the chat allowlist ─────────────────────────────────────────────────────
# D-decision: the identity gate is a fail-closed chat-id allowlist. Without it the bot answers
# anyone who finds it, and this one types into real terminals.
say
say "herdr-tg only answers chat ids on its allowlist. Everything else is dropped."
CHAT_ID=""
if [ -n "$BOT_USERNAME" ]; then
  printf 'Discover your chat id now? (you will send the bot a message) [Y/n] '
  read -r ans
  case "$ans" in
    [Nn]*) ;;
    *)
      say
      say "  1. Open Telegram and message @${BOT_USERNAME} — any text will do."
      say "  2. Come back here and press Enter."
      printf '  waiting... '
      read -r _
      updates=$(api getUpdates "$TOKEN" --get -d limit=10) || die "getUpdates failed"
      CHAT_ID=$(printf '%s' "$updates" |
        sed -n 's/.*"chat":{"id":\(-\?[0-9]\+\).*/\1/p' | head -1)
      if [ -n "$CHAT_ID" ]; then
        say "  ✅ found chat id: $CHAT_ID"
      else
        say "  no message seen yet — you can add the id by hand later"
      fi
      ;;
  esac
fi
if [ -z "$CHAT_ID" ]; then
  printf 'Chat id (leave empty to fill in later): '
  read -r CHAT_ID
fi

# ── 4. write it ───────────────────────────────────────────────────────────────
mkdir -p "$ENV_DIR"
chmod 700 "$ENV_DIR"

if [ -f "$ENV_FILE" ]; then
  backup="$ENV_FILE.$(date +%Y%m%d-%H%M%S).bak"
  cp -p "$ENV_FILE" "$backup"
  chmod 600 "$backup"
  say
  say "Existing file backed up to $backup"
fi

# umask before creation, so the token is never briefly world-readable.
( umask 077
  tmp="$ENV_FILE.tmp.$$"
  {
    printf '# herdr-tg credentials — written by scripts/setup-token.sh\n'
    printf '# Read by the systemd --user unit via EnvironmentFile=. Never commit this file.\n'
    printf 'HERDR_TG_TOKEN=%s\n' "$TOKEN"
    [ -n "$CHAT_ID" ] && printf 'HERDR_TG_ALLOWED_CHAT_IDS=%s\n' "$CHAT_ID"
    [ -n "$BOT_USERNAME" ] && printf '# bot: @%s\n' "$BOT_USERNAME"
  } > "$tmp"
  mv -f "$tmp" "$ENV_FILE"
)
chmod 600 "$ENV_FILE"

unset TOKEN

say
say "──────────────────────────────────────────────────────────────────"
say "Wrote $ENV_FILE"
ls -l "$ENV_FILE" | sed 's/^/  /'
say
say "Contents, with the token masked:"
sed 's/^\(HERDR_TG_TOKEN=\).*/\1<redacted>/' "$ENV_FILE" | sed 's/^/  /'
say
say "Nothing else needs doing. Tell the coordinator the file exists —"
say "it can check that the token LOADS without ever reading its value."
