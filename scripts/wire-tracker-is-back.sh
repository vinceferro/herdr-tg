#!/usr/bin/env bash
# wire-tracker-is-back.sh — KICKOFF.local.md says the tracker is dead. It is not, any more.
#
# The pin moved to core-v1.0.1-alpha, which ships mission-control/. `mc show` exits 0. The line
# telling every future session to avoid the tracker is now false, and a false charter line is worse
# than a missing one. Gated because it is a charter edit; running this IS the approval.
#
#   bash scripts/wire-tracker-is-back.sh          # apply
#   bash scripts/wire-tracker-is-back.sh --revert # undo, byte-for-byte
set -euo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)"
B=".kickoff/backups/tracker-is-back"
F=".kickoff/KICKOFF.local.md"
if [ "${1:-}" = "--revert" ]; then
  [ -f "$B/KICKOFF.local.md" ] || { echo "no backup at $B" >&2; exit 1; }
  cp "$B/KICKOFF.local.md" "$F"; echo "  reverted $F"; exit 0
fi
mkdir -p "$B"; [ -f "$B/KICKOFF.local.md" ] || cp "$F" "$B/KICKOFF.local.md"
python3 - <<'PY'
import pathlib, sys
p = pathlib.Path(".kickoff/KICKOFF.local.md"); s = p.read_text()
if "the tracker works again" in s:
    print("  already applied"); sys.exit(0)
old = """- **Report in chat, not to the tracker.** `.kickoff/bin/mc` is dead here — the pinned core dropped
  mission-control from the public line."""
new = """- **The tracker works again — use it.** `.kickoff/bin/mc` was dead for about ten hours on
  2026-08-31 (the pin sat on a core line that drops mission-control). The pin moved to
  core-v1.0.1-alpha, which ships it, and `mc show` exits 0. Keep the board current. The durable
  lesson is not about Mission Control: a pin can move under a running session, twice in one day, so
  check which core you are pinned to before concluding anything about a seam."""
if old not in s:
    print("  anchor not found — refusing to guess", file=sys.stderr); sys.exit(1)
p.write_text(s.replace(old, new, 1)); print("  applied: the tracker-is-dead line is corrected")
PY
echo "  backup: $B  (revert with --revert)"
