#!/usr/bin/env bash
# Install kickoff-hub-attach — the shim, and the systemd --user template. And PROVE it first.
#
#     bash scripts/install-attach.sh
#
# It writes ~/.local/bin/kickoff-hub-attach (a two-line shim onto this repo's main.ts), copies the
# unit template to ~/.config/systemd/user/, and runs daemon-reload. It STARTS NOTHING — the same
# discipline as install-channel-plugin.sh: it proves the thing works, then leaves the operator to
# enable a worker when he has one. Opening a door for a conversation is a live connection to the hub,
# and an installer has no business making one.
#
# Idempotent: re-running rewrites the shim and reinstalls the unit.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAIN="$REPO/adapters/kickoff-hub-attach/main.ts"
BIN_DIR="$HOME/.local/bin"
BIN_DST="$BIN_DIR/kickoff-hub-attach"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
UNIT_SRC="$REPO/deploy/kickoff-hub-attach@.service"
UNIT_DST="$UNIT_DIR/kickoff-hub-attach@.service"

say() { printf '%s\n' "$*"; }
die() { printf '\nerror: %s\n' "$*" >&2; exit 1; }

say "kickoff-hub-attach — install"
say "──────────────────────────────────────────────────────────────────"

command -v bun >/dev/null || die "bun is not on PATH — attach runs on it"
[ -f "$MAIN" ] || die "no entrypoint at $MAIN"
[ -f "$UNIT_SRC" ] || die "no unit template at $UNIT_SRC"

# An agent session on this box inherits TMPDIR as the literal string "%h/.cache/tmp" — an
# unexpanded systemd specifier that is not a path — which the sun_path-capped socket tests fail on.
# Give the proof a real, short TMPDIR the way CLAUDE.md documents for cargo.
case "${TMPDIR:-/tmp}" in
  /*) ;;
  *)  TMPDIR=/tmp; export TMPDIR ;;
esac

# Prove it before installing it, against fakes only — no hub is touched, no topic is made, and the
# operator's phone is not a test fixture. A green line here means the door, the watcher, --check and
# --run all agree with the wire.
say
say "Proving attach against fakes…"
(
  cd "$REPO/adapters/kickoff-hub-attach"
  bun test-two-producers.ts >/dev/null &&
  bun test-what-breaks-it.ts >/dev/null &&
  bun test-against-fakes.ts >/dev/null &&
  bun test-check.ts >/dev/null &&
  bun test-run.ts >/dev/null
) || die "attach failed its own tests; not installing it"
say "  ✓ the door, the watcher, --check and --run all pass"

# The shim: two lines onto this repo's entrypoint, so the unit and the operator type one word.
mkdir -p "$BIN_DIR"
cat > "$BIN_DST" <<SHIM
#!/usr/bin/env bash
exec bun "$MAIN" "\$@"
SHIM
chmod 0755 "$BIN_DST"
say
say "  installed $BIN_DST"

mkdir -p "$UNIT_DIR"
install -m 0644 "$UNIT_SRC" "$UNIT_DST"
systemctl --user daemon-reload
say "  installed $UNIT_DST (a template; it starts nothing)"

say
say "A worker is one command. On this box, in the worktree it is for:"
say
say "    KICKOFF_HUB_PROJECT_DIR=. kickoff-hub-attach --check --opencode http://127.0.0.1:9711 --run opencode serve --port 9711"
say "    KICKOFF_HUB_PROJECT_DIR=. kickoff-hub-attach         --opencode http://127.0.0.1:9711 --run opencode serve --port 9711"
say
say "Or supervised, per worker — write ~/.config/kickoff-hub-attach/<label>.env with"
say "KICKOFF_HUB_PROJECT_DIR and OPENCODE_PORT — and, where a launcher writes the file naming"
say "which session of that server the worker is, OPENCODE_BINDING_FILE and OPENCODE_BINDING_GENERATION"
say "beside them — then:"
say
say "    systemctl --user enable --now kickoff-hub-attach@<label>"
say "    journalctl --user -u kickoff-hub-attach@<label> -f"
say
say "Enrolment stays a terminal act, once per project:  kickoff-channel enroll <repo>"
