#!/usr/bin/env bash
# Register this repo as a plugin marketplace and install `kickoff-channel` from it.
#
#     bash scripts/install-channel-plugin.sh
#
# WHY OUR OWN MARKETPLACE, and not kickoff's. `kickoff-local` points at
# ~/kickoff-versions/<pinned-core>/plugin — the PINNED CORE, which an engine hop replaces wholesale.
# A plugin living there is a plugin the next `kickoff pull` deletes. This one lives in the repo that
# maintains it.
#
# The one thing this script cannot do is the door: `channelsEnabled` is only read from managed
# settings, which is /etc/claude-code/managed-settings.json and needs root. It prints the exact
# command and stops.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLUGIN="$REPO/plugins/kickoff-channel"
MANAGED="/etc/claude-code/managed-settings.json"

say() { printf '%s\n' "$*"; }
die() { printf '\nerror: %s\n' "$*" >&2; exit 1; }

say "kickoff-channel — install"
say "──────────────────────────────────────────────────────────────────"

command -v claude >/dev/null || die "claude is not on PATH"
command -v bun >/dev/null || die "bun is not on PATH — the plugin runs on it"
[ -f "$REPO/.claude-plugin/marketplace.json" ] || die "no marketplace manifest at $REPO/.claude-plugin"
[ -f "$PLUGIN/server.ts" ] || die "no plugin at $PLUGIN"

# Prove the bridge works before installing it anywhere. It talks to a fake hub over a real socket,
# so a green line here means the framing, the handshake and the tools are right — everything except
# the door.
say
say "Proving the bridge against a fake hub…"
( cd "$PLUGIN" && bun install --no-summary >/dev/null && bun test-against-a-fake-hub.ts ) \
  || die "the bridge failed its own test; not installing it"

say
claude plugin marketplace add "$REPO" 2>&1 | sed 's/^/  /' || true
claude plugin install kickoff-channel@herdr-tg-local 2>&1 | sed 's/^/  /' || true

say
if [ -f "$MANAGED" ] && grep -q '"channelsEnabled"' "$MANAGED" 2>/dev/null; then
  say "✅ managed settings already enable channels: $MANAGED"
else
  say "⚠ THE DOOR IS STILL SHUT, and only root can open it."
  say
  say "  Channels are read from managed settings and nowhere else. Run:"
  say
  say "    sudo mkdir -p /etc/claude-code"
  say "    sudo install -m 0644 $REPO/deploy/managed-settings.json $MANAGED"
  say
  say "  That file re-lists the OFFICIAL telegram plugin alongside ours on purpose: an org"
  say "  allowlist REPLACES the default, so leaving it out makes every project that still uses"
  say "  the official channel go deaf on the same day."
fi

say
say "Then, per project:"
say "    herdr-tg enroll <repo>"
say "    echo 'CHANNEL_SPEC=plugin:kickoff-channel@herdr-tg-local' >> <repo>/.kickoff/instance.env"
