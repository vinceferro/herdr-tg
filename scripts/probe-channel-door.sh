#!/usr/bin/env bash
# probe-channel-door.sh — does a Claude Code CHANNEL plugin load on this machine at all?
#
# WHY THIS EXISTS. docs/HUB-DESIGN.md rests entirely on one mechanism: a worker started with
# `--channels plugin:<name>@<marketplace>` loads that plugin and talks to it. Nothing on this box has
# ever used it. `~/.claude/channels/` is empty, `/etc/claude-code/` does not exist, and `CHANNEL_SPEC`
# is COMMENTED OUT in every repo that mentions it — your phone reaches workers today through the
# standalone bun bridge that polls Telegram directly, which is a different mechanism entirely.
#
# The adversarial review then found the gate is not one switch but two, and the outer one is worse:
# `tengu_harbor` defaults to FALSE, is checked BEFORE any org policy, and no managed setting or env
# var on this box reaches it. If it is off, slice 1 cannot be built and no amount of local config
# changes that. This costs ~30 seconds to find out and it is the cheapest thing that can kill the
# design, so it runs before anything is written.
#
# WHAT IT DOES. Starts one throwaway `claude` exactly the way session-run.sh starts a worker — same
# --channels flag, same --plugin-dir, same never-EOF stdin — for a few seconds, captures what it says
# about the channel, and kills it. It writes NOTHING: no instance.env change, no settings change, no
# supervisor, no bot token, no message sent anywhere. There is nothing to revert.
#
#   bash scripts/probe-channel-door.sh
#
# It probes with the OFFICIAL telegram plugin, which is already in the marketplace here. That is
# deliberate: the question is whether the channel MACHINERY works, not whether our future plugin does,
# and using a plugin that already exists keeps the answer about the gate.

set -uo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)"

SPEC="plugin:telegram@claude-plugins-official"
OUT="${TMPDIR:-/tmp}/channel-door-probe.$$"
SECS=25

command -v claude >/dev/null 2>&1 || { echo "claude is not on PATH — run this from a shell that has it" >&2; exit 2; }

CORE=""
[ -f .kickoff/instance.env ] && CORE="$(. .kickoff/instance.env >/dev/null 2>&1; printf '%s' "${KICKOFF_CORE_DIR:-}")"
PLUGIN_ARGS=()
if [ -n "$CORE" ] && [ -d "$CORE/plugin" ]; then
  PLUGIN_ARGS=(--plugin-dir "$CORE/plugin")
  echo "  using --plugin-dir $CORE/plugin   (same as a real worker)"
fi

echo "  probing with $SPEC for ${SECS}s — nothing is written, nothing is sent"
echo

# stdin that never EOFs, exactly as session-run.sh does: an interactive --channels session exits
# immediately on a closed stdin, and that exit would read as a channel failure when it is not one.
timeout "$SECS" claude \
  --channels "$SPEC" \
  "${PLUGIN_ARGS[@]}" \
  --permission-mode default \
  < <(tail -f /dev/null) > "$OUT" 2>&1
rc=$?

echo "── what it said about the channel ─────────────────────────────────────────"
grep -i -m 12 -E 'channel|harbor|marketplace|plugin.*(skip|reject|unavailable|not currently)' "$OUT" \
  | sed 's/^/  /' || echo "  (nothing matched — see the full capture below)"
echo

verdict="UNCLEAR"
if   grep -qi 'not currently available' "$OUT"; then
  verdict="DOOR SHUT — the master gate is off"
  echo "  VERDICT: the channels feature is not available to this account."
  echo "  No local setting reaches that gate. Slice 1 cannot be built until it changes."
  echo "  The honest fallback is the one-day change: keep the read path, disable the write path."
elif grep -qiE 'you asked for plugin:.*but the installed' "$OUT"; then
  verdict="DOOR OPEN, wrong key"
  echo "  VERDICT: the gate let us through and the MARKETPLACE check rejected the spec."
  echo "  That is the good outcome: channels work, and the design's own warning about"
  echo "  marketplace resolution is confirmed rather than theoretical."
elif grep -qi 'channel' "$OUT"; then
  verdict="DOOR OPEN"
  echo "  VERDICT: a channel was resolved. The mechanism slice 1 needs works here."
else
  echo "  VERDICT: unclear — the run said nothing about channels."
  echo "  That is itself a finding: a silently skipped channel is the failure mode the"
  echo "  design's Risk 1 names, and it looks exactly like this."
fi

echo
echo "  exit code from claude: $rc   (124 = the timeout fired, which is expected and fine)"
echo "  full capture kept at: $OUT"
echo "  verdict: $verdict"
