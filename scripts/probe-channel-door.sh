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
# The pty rendering collapses inter-word spaces, so the banner reads "pluginnotinstalled" and not
# "plugin not installed". Match against a whitespace-free copy — the fourth bug in this script and
# the fourth of the same kind: matching text I assumed rather than the text that is actually there.
SQUEEZED="$OUT.squeezed"
tr -d '[:space:]' < "$PLAIN" > "$SQUEEZED"

echo "── what it said about the channel ─────────────────────────────────────────"
grep -i -m 12 -E '/rc|channel|harbor|marketplace|connect|plugin.*(skip|reject|unavailable|not currently)' "$PLAIN" \
  | sed 's/^/  /' || echo "  (nothing matched — see the full capture below)"
echo

verdict="UNCLEAR"
# The session prints a real banner for --channels, marked with U+258E, and it names a reason per
# spec. That banner IS the signal. Two earlier versions of this script looked for the word
# "channel" and for "/rc" instead, and "/rc" turned out to be REMOTE CONTROL — it appears with no
# --channels flag at all, proven by a control run. Do not read it as evidence about channels.
if   grep -qi 'notcurrentlyavailable' "$SQUEEZED"; then
  verdict="DOOR SHUT — the master gate is off"
  echo "  VERDICT: the channels feature is not available to this account. No local setting reaches"
  echo "  that gate. Slice 1 cannot be built until it changes, and the honest fallback is the"
  echo "  one-day change: keep the read path, disable the write path."
elif ! grep -q 'Channels' "$SQUEEZED"; then
  verdict="NO ANSWER — the flag never took effect"
  echo "  VERDICT: no channels banner appeared at all, so --channels did nothing. That is a broken"
  echo "  probe, not a shut door — check the script(1) wrapper and the spec string."
elif grep -qi 'notontheapprovedchannelsallowlist' "$SQUEEZED"; then
  verdict="GATE ON, allowlist rejects this plugin"
  echo "  VERDICT: the channels feature IS available — the banner rendered and named a reason."
  echo "  This particular plugin is not on the approved allowlist. That is the org-policy gate the"
  echo "  design proposes to configure, and it is behaving exactly as documented."
elif grep -qi 'pluginnotinstalled' "$SQUEEZED"; then
  verdict="DOOR OPEN — the plugin just is not installed here"
  echo "  VERDICT: the channels feature IS available to this account, and the gate speaks clearly."
  echo "  The only complaint is that this plugin is not installed for this repo — and NOT that it"
  echo "  is off the allowlist, which means the official plugin is allowlisted by default, as the"
  echo "  design assumed. The mechanism slice 1 needs works here."
  echo
  echo "  It also retires the design's Risk 1 fear of a SILENT skip: the refusal is a banner in the"
  echo "  session, naming the spec and the reason, three lines from the top."
else
  verdict="DOOR OPEN — channel resolved"
  echo "  VERDICT: the banner rendered with no refusal. The channel resolved."
fi

echo
echo "  exit code from claude: $rc   (124 = the timeout fired, which is expected and fine)"
echo "  full capture: $OUT   (stripped: $PLAIN)"
echo "  verdict: $verdict"
