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
# NOT $TMPDIR: this box hands agent sessions the literal string "%h/.cache/tmp", an unexpanded
# systemd specifier that resolves relative to wherever you happen to be. The first version of this
# script used it and wrote its capture into the repo. See .kickoff/memory/cargo-needs-a-real-tmpdir.md.
OUT="$(env -u TMPDIR mktemp /tmp/channel-door-probe.XXXXXX)"  # -u TMPDIR: mktemp honours it, and here it is broken
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

# TWO things a real worker has that a naive invocation does not, and BOTH are load-bearing:
#
#   1. A PTY. Without one, claude decides it is non-interactive and falls back to --print, which
#      needs a prompt and errors out before it ever looks at --channels. The first version of this
#      script hit exactly that and reported "unclear" about a question it never asked.
#      session-run.sh:422 re-execs itself through script(1) for this reason.
#   2. Stdin that never EOFs. An interactive session exits the moment stdin closes, and that exit
#      would read as a channel failure when it is not one.
#
# So: script(1) supplies the pty, `tail -f /dev/null` supplies the never-ending stdin.
CMD="claude --channels '$SPEC'"
for a in ${PLUGIN_ARGS+"${PLUGIN_ARGS[@]}"}; do CMD="$CMD '$a'"; done
CMD="$CMD --permission-mode default"

timeout "$SECS" script -qfe -c "$CMD" /dev/null < <(tail -f /dev/null) > "$OUT" 2>&1
rc=$?

# The capture is a PTY recording: it is dense with ANSI escapes and carriage returns, and a plain
# grep over it matches almost nothing. The first two runs of this script reported "unclear" partly
# for that reason. Strip first, then read.
PLAIN="$OUT.plain"
sed 's/\x1b\[[0-9;?]*[a-zA-Z]//g; s/\x1b[()][A-Z0-9]//g' "$OUT" | tr -d '\r' | grep -v '^[[:space:]]*$' > "$PLAIN"

echo "── what it said about the channel ─────────────────────────────────────────"
grep -i -m 12 -E '/rc|channel|harbor|marketplace|connect|plugin.*(skip|reject|unavailable|not currently)' "$PLAIN" \
  | sed 's/^/  /' || echo "  (nothing matched — see the full capture below)"
echo

verdict="UNCLEAR"
if   grep -qi 'not currently available' "$PLAIN"; then
  verdict="DOOR SHUT — the master gate is off"
  echo "  VERDICT: the channels feature is not available to this account."
  echo "  No local setting reaches that gate. Slice 1 cannot be built until it changes."
  echo "  The honest fallback is the one-day change: keep the read path, disable the write path."
elif grep -qiE 'you asked for plugin:.*but the installed' "$PLAIN"; then
  verdict="DOOR OPEN, wrong key"
  echo "  VERDICT: the gate let us through and the MARKETPLACE check rejected the spec."
  echo "  That is the good outcome: channels work, and the design's own warning about"
  echo "  marketplace resolution is confirmed rather than theoretical."
elif grep -qiE 'must be provided|--print' "$PLAIN"; then
  verdict="PROBE BROKEN — no pty"
  echo "  VERDICT: the probe failed, not the door. claude fell back to --print, which means it"
  echo "  never had a terminal and never looked at --channels. The script is supposed to prevent"
  echo "  this; if you see it, the script(1) wrapper is not working and this tells you nothing."
elif grep -qiE '/rc|channel' "$PLAIN"; then
  verdict="DOOR OPEN"
  echo "  VERDICT: the channel subsystem engaged — the status line carries the remote-channel"
  echo "  indicator and NOTHING was skipped or rejected. The master gate is on for this account"
  echo "  and the mechanism slice 1 needs exists here."
  echo
  echo "  What this does NOT prove: that a channel fully CONNECTED. The official telegram plugin"
  echo "  needs a bot token this probe deliberately does not supply, so \"connecting…\" is the"
  echo "  expected resting state. The gate was the question; the gate is open."
else
  echo "  VERDICT: unclear — the run said nothing about channels."
  echo "  That is itself a finding: a silently skipped channel is the failure mode the"
  echo "  design's Risk 1 names, and it looks exactly like this."
fi

echo
echo "  exit code from claude: $rc   (124 = the timeout fired, which is expected and fine)"
echo "  full capture: $OUT   (ANSI-stripped: $PLAIN)"
echo "  verdict: $verdict"
