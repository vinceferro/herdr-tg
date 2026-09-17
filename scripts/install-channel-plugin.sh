#!/usr/bin/env bash
# Register this repo as a plugin marketplace and put `kickoff-channel` on this box — reproducibly.
#
#     bash scripts/install-channel-plugin.sh [--allow-dirty]
#
# WHY OUR OWN MARKETPLACE, and not kickoff's. `kickoff-local` points at
# ~/kickoff-versions/<pinned-core>/plugin — the PINNED CORE, which an engine hop replaces wholesale.
# A plugin living there is a plugin the next `kickoff pull` deletes. This one lives in the repo that
# maintains it.
#
# WHY THIS SCRIPT VERIFIES INSTEAD OF ANNOUNCING. Measured on a real box on 8 September 2026: the
# marketplace cache held a `server.ts` of 15262 bytes dated 1 September with no `hub-link.ts` beside
# it, while the repo's copy was 74665 bytes with `hub-link.ts` and three more files. Both declared
# version 0.1.0. The cache directory is keyed by the VERSION, this script only ever ran the install
# verb, and installing a version that is already present does nothing — so a week of fixes was never
# picked up, on every box that already had the plugin, and the old script printed a tick anyway
# because both `claude` calls ended in `|| true`. Nothing here swallows an exit code now, and
# nothing here calls the install done until the bytes on the box have been compared with the bytes
# in the repo, file by file — and until what is on the box that this repo does NOT track has been
# accounted for too, because a claim of "byte for byte" over a comparison that skipped half the
# directory is the same failure wearing a tick.
#
# What it put there is written down at `plugin.installed` in the hub's own state directory, outside
# every repository, so tomorrow's question — which bridge is this box carrying — has an answer that
# does not depend on someone having kept this terminal open.
#
# The one thing this script cannot do is the door: `channelsEnabled` is only read from managed
# settings, which is /etc/claude-code/managed-settings.json and needs root. It prints the exact
# command and stops.

set -euo pipefail

# `pwd -P` and not `pwd`: `claude plugin marketplace add` records the path it was GIVEN, so one run
# through a symlink and the next through the real path would name two directories that are one
# checkout — and the script would tell the operator his own tree belonged to somebody else, with a
# `remove` command that would undo a perfectly correct registration.
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
PLUGIN_REL="plugins/kickoff-channel"
PLUGIN="$REPO/$PLUGIN_REL"
# The door lives under /etc and needs root, so the only way to exercise BOTH branches of the check
# below is to be able to point it somewhere else. A branch nobody can drive is a branch that stays
# wrong, and this one did: it used to read a `false` as an open door.
MANAGED="${HERDR_TG_MANAGED_SETTINGS:-/etc/claude-code/managed-settings.json}"
STATE="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/plugins"
KNOWN="$STATE/known_marketplaces.json"
INSTALLED="$STATE/installed_plugins.json"
# What is on this box, written outside every repository — hub state in a working tree is state an
# adopter's coordinator commits and pushes. This is the file another program reads to find out
# which bridge a box is carrying; the terminal that ran the install is not a record.
RECORD_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/herdr-tg"
RECORD="$RECORD_DIR/plugin.installed"

ALLOW_DIRTY=no
for arg in "$@"; do
  case "$arg" in
    --allow-dirty) ALLOW_DIRTY=yes ;;
    *) printf 'error: I do not know the option %s\n' "$arg" >&2; exit 1 ;;
  esac
done

say() { printf '%s\n' "$*"; }
die() { printf '\nerror: %s\n' "$*" >&2; exit 1; }

# Where scratch goes, checked before anything uses it. An agent session on this box inherits TMPDIR
# as the literal string `%h/.cache/tmp` — no directory of that name exists, and `mktemp` then failed
# with a sentence about templates and nothing about what to change.
SCRATCH="${TMPDIR:-/tmp}"
if [ ! -d "$SCRATCH" ] || [ ! -w "$SCRATCH" ]; then
  printf '\nerror: there is nowhere to work: TMPDIR names %s, and that is not a directory this account can write to. Set TMPDIR to a real absolute directory, or unset it and /tmp is used.\n' "$SCRATCH" >&2
  exit 1
fi

# Where the wire proof may put its scratch, which is a different question. That proof binds a Unix
# socket, and a socket path is 108 bytes all in — the harness spends about sixty of them on names it
# chooses itself, so a longer TMPDIR than this leaves no room, the bind fails, and the failure is
# reported as "the bridge and the hub disagree about the wire": a false accusation against a bridge
# that is entirely correct. This box's own default TMPDIR is over the line.
PROOF_TMPDIR="$SCRATCH"
if [ "${#PROOF_TMPDIR}" -gt 40 ]; then PROOF_TMPDIR=/tmp; fi
if [ ! -d "$PROOF_TMPDIR" ] || [ ! -w "$PROOF_TMPDIR" ]; then
  printf '\nerror: the bridge proof needs somewhere short to put a socket, and %s is not a directory this account can write to.\n' "$PROOF_TMPDIR" >&2
  exit 1
fi

# One scratch directory for the output of the commands we run, cleaned up however we leave. The
# output of a failed command is the only thing that tells a person what went wrong, so it is
# captured and printed rather than sent to /dev/null.
WORK="$(mktemp -d "$SCRATCH/kickoff-channel-install.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

# Print a captured log under the line that says which command produced it.
#
# `|| [ -n "$line" ]` because plenty of tools end their last line without a newline, and a bare
# `read` throws that line away — the one line that says why, out of output captured for no other
# reason than to say why.
show() {
  local file="$1" line
  while IFS= read -r line || [ -n "$line" ]; do printf '  | %s\n' "$line"; done < "$file"
}

# Run a command, capture everything it says, and stop the script if it did not end cleanly.
run() {
  local what="$1"; shift
  local log="$WORK/step.log" rc=0
  "$@" > "$log" 2>&1 || rc=$?
  if [ "$rc" -ne 0 ]; then
    say
    say "$what failed (exit $rc). It said:"
    show "$log"
    die "$what failed; nothing was installed"
  fi
  show "$log"
}

# A `claude plugin …` call, under a deadline. Only these get one: a cold `bun install` or a first
# build of this workspace legitimately takes longer than two minutes, and killing either would look
# exactly like the failure this script exists to report.
#
# STDIN IS CLOSED, and the deadline is five minutes rather than two. Measured on 9 September: the
# update step ran to 120 seconds and was killed, reported as "did not finish within two minutes",
# and the operator read that as the plugin being broken. Run again with stdin at /dev/null it
# finished in under a hundred seconds and said what it had done. A command that inherits a terminal
# can wait on a person who is not there, and a deadline that sits near the honest running time
# turns a slow success into a false failure — so close the one and widen the other.
run_claude() {
  local what="$1"; shift
  local log="$WORK/step.log" rc=0
  timeout 300 claude "$@" < /dev/null > "$log" 2>&1 || rc=$?
  if [ "$rc" -ne 0 ]; then
    say
    if [ "$rc" -eq 124 ]; then
      say "$what did not finish within five minutes. It said:"
    else
      say "$what failed (exit $rc). It said:"
    fi
    show "$log"
    die "$what failed; nothing was installed"
  fi
  show "$log"
}

# Print, line by line, where the two fingerprints disagree. `diff` answers 1 when the files differ,
# and this script runs under `pipefail` — so a diff in a pipeline ENDS THE SCRIPT silently, right
# where it was about to explain itself. That is why the comparison is spelled out here, once, with
# its exit code read: 0 and 1 are answers, anything above is diff failing and must never read as
# "no differences".
show_difference() {
  local a="$1" b="$2" rc=0 line
  diff "$a" "$b" > "$WORK/difference" || rc=$?
  if [ "$rc" -gt 1 ]; then
    die "I could not compare the copy on this box with the copy in this repo, so I cannot say it is right"
  fi
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      '< '*) printf '    on this box   %s\n' "${line#< }" ;;
      '> '*) printf '    in this repo  %s\n' "${line#> }" ;;
    esac
  done < "$WORK/difference"
}

say "kickoff-channel — install"
say "──────────────────────────────────────────────────────────────────"

# ── 1. everything this script needs, named before anything is touched ────────────────────────────
for tool in claude bun git jq sha256sum diff timeout find date; do
  command -v "$tool" >/dev/null || die "$tool is not on PATH, and this install cannot be checked without it"
done
[ -f "$REPO/.claude-plugin/marketplace.json" ] || die "no marketplace manifest at $REPO/.claude-plugin"
[ -f "$PLUGIN/server.ts" ] || die "no plugin at $PLUGIN"
[ -f "$PLUGIN/.claude-plugin/plugin.json" ] || die "no plugin manifest at $PLUGIN/.claude-plugin"

MARKET="$(jq -r '.name // empty' "$REPO/.claude-plugin/marketplace.json")"
[ -n "$MARKET" ] || die "the marketplace manifest does not say what this marketplace is called"
PLUGIN_NAME="$(jq -r '.name // empty' "$PLUGIN/.claude-plugin/plugin.json")"
[ -n "$PLUGIN_NAME" ] || die "the plugin manifest does not say what this plugin is called"
SPEC="$PLUGIN_NAME@$MARKET"

# Everything below fingerprints $PLUGIN. If the marketplace points somewhere else, we would check one
# directory and install another, and the check would pass while the box got something we never read.
LISTED="$(jq -r --arg n "$PLUGIN_NAME" '.plugins[]? | select(.name == $n) | .source' "$REPO/.claude-plugin/marketplace.json")"
[ "$LISTED" = "./$PLUGIN_REL" ] \
  || die "the marketplace offers $PLUGIN_NAME from '$LISTED', and this script checks ./$PLUGIN_REL; they must be the same directory"

# The same checkout under another name is the same checkout. Both sides of every path comparison
# below go through this first, so a symlink cannot make one tree look like two.
resolved() {
  ( cd "$1" 2>/dev/null && pwd -P ) || printf '%s\n' "$1"
}

# ── 2. one version, spelled in two files ─────────────────────────────────────────────────────────
# The version is the cache key. Two manifests carry it and they drift silently, so they are compared
# before anything else: shipping a bump in one of them busts nothing.
VERSION="$(jq -r '.version // empty' "$PLUGIN/.claude-plugin/plugin.json")"
PKG_VERSION="$(jq -r '.version // empty' "$PLUGIN/package.json")"
[ -n "$VERSION" ] || die "the plugin manifest has no version, and the version is what makes a new copy land"
if [ "$VERSION" != "$PKG_VERSION" ]; then
  die "the plugin says version $VERSION in one manifest and $PKG_VERSION in the other; they are one version"
fi

# ── 3. the tree that is about to be copied ───────────────────────────────────────────────────────
# What a directory marketplace installs is the working tree, not the last commit. So an uncommitted
# edit here is an uncommitted edit on the box, and "installed <commit>" would be a lie.
git -C "$REPO" rev-parse --git-dir > /dev/null 2>&1 \
  || die "$REPO is not a git checkout, so there is no way to say which copy of the bridge got installed"

# sha256 of every tracked file in a directory, one `<hash>  <path>` line each. A file the directory
# does not have becomes a line saying so, so a missing file can never read as a match.
fingerprint() {
  local dir="$1" rel
  while IFS= read -r rel || [ -n "$rel" ]; do
    if [ -f "$dir/$rel" ]; then
      printf '%s  %s\n' "$(sha256sum < "$dir/$rel" | cut -d' ' -f1)" "$rel"
    else
      printf '%s  %s\n' "MISSING---------------------------------------------------------" "$rel"
    fi
  done < "$WORK/files"
}

# Every file in a directory that the fingerprint above never looked at, `node_modules/` aside.
#
# WHY THIS EXISTS. The fingerprint is taken over `git ls-files`, and a directory marketplace copies
# the WHOLE plugin directory — measured against the real `claude` on 8 September 2026, gitignored
# files and all. So a file the repo does not track is a file neither side ever compared, and
# printing "N of N match, byte for byte" over a box that also holds bytes nobody read is exactly the
# announcing-instead-of-verifying this script was rewritten to stop. `node_modules/` is the one
# untracked thing that BELONGS there — `bun.lock` is tracked and is what says what it must contain —
# so it is excluded here and named in the verified block instead of being silently included in a
# claim about bytes.
untracked_on_the_box() {
  local root="$1" rel
  ( cd "$root" && find . -type f ) | sed 's|^\./||' | while IFS= read -r rel || [ -n "$rel" ]; do
    case "$rel" in node_modules/*) continue ;; esac
    grep -qxF -- "$rel" "$WORK/files" || printf '%s\n' "$rel"
  done
}

# Read the working tree: refuse it if it is not committed, then write down exactly which bytes an
# install would copy. Called twice — once now, so a dirty tree is refused before anyone waits on the
# proofs, and once after them, because `bun install` writes a lockfile this repo tracks and a
# fingerprint taken before it would describe a tree that no longer exists.
survey_tree() {
  local into="$1"
  DIRTY="$(git -C "$REPO" status --porcelain -- "$PLUGIN_REL")"
  if [ -n "$DIRTY" ] && [ "$ALLOW_DIRTY" != yes ]; then
    say
    say "These files under $PLUGIN_REL are not committed:"
    printf '%s\n' "$DIRTY" | while IFS= read -r line || [ -n "$line" ]; do printf '  %s\n' "$line"; done
    say
    die "the copy this would install is not a copy anyone can get back to; commit it, or re-run with --allow-dirty"
  fi
  COMMIT="$(git -C "$REPO" rev-parse --short HEAD)"
  [ -n "$COMMIT" ] || die "this checkout has no commits, so there is no way to say which copy got installed"
  if [ -n "$DIRTY" ]; then COMMIT="$COMMIT plus uncommitted edits"; fi

  # `git ls-files` is the whole list on both sides of the comparison — an untracked file would be
  # copied to the box and checked against nothing, which is why an untracked file makes the tree
  # dirty above.
  if ! git -C "$REPO" ls-files -- "$PLUGIN_REL" | sed "s|^$PLUGIN_REL/||" > "$WORK/files"; then
    die "I could not list the files this repo tracks under $PLUGIN_REL, so I cannot check what gets installed"
  fi
  FILE_COUNT="$(wc -l < "$WORK/files" | tr -d ' ')"
  [ "$FILE_COUNT" -gt 0 ] || die "no tracked files under $PLUGIN_REL, so there is nothing to install"

  fingerprint "$PLUGIN" > "$into"
  if grep -q '^MISSING' "$into"; then
    die "a file this repo tracks is not on disk; the checkout is incomplete"
  fi
}

survey_tree "$WORK/before.sha"

# ── 4. prove the bridge before putting it anywhere ───────────────────────────────────────────────
# It talks to a fake hub over a real socket, so a green line here means the framing, the handshake
# and the tools are right — everything except the door.
say
say "Proving the bridge against a fake hub…"
run "the bridge's dependency install" bash -c 'cd "$1" && bun install --no-summary' _ "$PLUGIN"
run "the bridge's own test" bash -c 'cd "$1" && bun test-against-a-fake-hub.ts' _ "$PLUGIN"

# And the other direction: the REAL bridge against the REAL hub, over a real socket, with only
# Telegram faked. Both halves passing against a fake of the other proves less than it looks — each
# fake was written from the same reading of the spec, so a shared misreading survives both. It is
# `#[ignore]`d in the Rust suite because that suite has no business requiring bun, which means this
# script is the thing that keeps it from rotting.
say
say "Proving the real bridge against the real hub…"
if ! ( cd "$REPO" && env -u RUSTUP_TOOLCHAIN TMPDIR="$PROOF_TMPDIR" PATH="$HOME/.cargo/bin:$PATH" \
         cargo test -q -p kickoff-channel the_real_plugin -- --ignored > "$WORK/wire.log" 2>&1 ); then
  say
  say "It said:"
  show "$WORK/wire.log"
  die "the bridge and the hub disagree about the wire; not installing it"
fi
show "$WORK/wire.log"

# And the promise the boxes that are NOT being installed to depend on: a bridge from before this
# change still works against this hub. It is a separate run because its name shares no substring
# with the filter above — one filter covered only the first proof, and an install could be called
# verified with this one never once executed. It is slower than every other proof here (about twenty
# seconds), which is why it is named rather than folded into a wider filter that would also pull in
# the proxy-driven child.
say
say "Proving a bridge from before this change still works against this hub…"
if ! ( cd "$REPO" && env -u RUSTUP_TOOLCHAIN TMPDIR="$PROOF_TMPDIR" PATH="$HOME/.cargo/bin:$PATH" \
         cargo test -q -p kickoff-channel -- --ignored --exact \
         hub::tests::a_bridge_from_before_this_change_still_works_against_the_new_hub \
         > "$WORK/older.log" 2>&1 ); then
  say
  say "It said:"
  show "$WORK/older.log"
  die "this hub turns away a bridge from before this change; every box already carrying one would go quiet. Not installing it"
fi
show "$WORK/older.log"

# The fleet trial is NOT run here. It proves four conversations do not cross, which is a property of
# the hub and the adapter: installing a bridge cannot break it and cannot fix it. It was here for a
# day, and the operator was right to pull it — an install that fails for something the install did
# not cause tells him his plugin is unsafe when his plugin is fine. It was put here because a
# fixture nothing runs rots; the answer to that is the org whose trial it is running it, not a gate
# on an unrelated door. `scripts/fleet-trial.sh` is the one way in, and it keeps the refusal that
# matters: a filter matching nothing is a pass to `cargo test`, so a trial renamed out of existence
# would otherwise report green having run nothing.

# The tree AFTER the proofs is the tree that gets copied, and it is also the tree the proofs ran
# against — `bun install` writes a lockfile this repo tracks, so a fingerprint taken before it would
# describe a tree that no longer exists. Reading it again here is what makes the verification at the
# end compare against the right bytes; if the lockfile did move, the clean-tree check inside this
# call is what refuses, by name.
survey_tree "$WORK/source.sha"
if ! diff -q "$WORK/before.sha" "$WORK/source.sha" > /dev/null; then
  say
  say "Proving the bridge rewrote files it is made of; what gets installed is the tree as it is now."
fi

# One short name for the bytes themselves. A commit names the bytes only when the tree is clean, so
# with --allow-dirty two boxes could hold different bridges and record the same identity — and
# nothing outside the terminal that ran the install would ever say so.
CONTENTS="$(sha256sum < "$WORK/source.sha" | cut -c1-12)"

# ── 5. the marketplace ───────────────────────────────────────────────────────────────────────────
# The marketplace name is global to this box. If it is already taken by another checkout, adding it
# again would silently re-point every session that uses it — so this refuses and says how to undo it.
REGISTERED_AT=""
REGISTERED_KIND=""
if [ -f "$KNOWN" ]; then
  jq -e 'type == "object"' > /dev/null < "$KNOWN" \
    || die "the list of marketplaces on this box is not a shape this script knows how to read; not touching it"
  REGISTERED_AT="$(jq -r --arg n "$MARKET" '.[$n].installLocation // .[$n].source.path // empty' "$KNOWN")"
  REGISTERED_KIND="$(jq -r --arg n "$MARKET" '.[$n].source.source // empty' "$KNOWN")"
fi
say
if [ -z "$REGISTERED_AT" ]; then
  say "Registering this checkout as the $MARKET marketplace…"
  run_claude "registering the marketplace" plugin marketplace add "$REPO"
elif [ "$(resolved "$REGISTERED_AT")" != "$REPO" ] || { [ -n "$REGISTERED_KIND" ] && [ "$REGISTERED_KIND" != directory ]; }; then
  say "The name $MARKET is already taken on this box:"
  say "    it points at   $REGISTERED_AT"
  say "    this checkout  $REPO"
  say
  say "  Both cannot hold it. If this checkout should own it, drop the other one first:"
  say
  say "    claude plugin marketplace remove $MARKET"
  say
  die "the $MARKET marketplace belongs to another checkout; nothing was changed"
else
  say "The $MARKET marketplace already points at this checkout."
fi

# ── 6. install, update, or refuse ────────────────────────────────────────────────────────────────
# The record of what is installed. A shape this script has not read before is a refusal, not a
# guess: everything after this point decides whether to install from what it says.
check_registry() {
  [ -f "$INSTALLED" ] || return 0
  jq -e 'type == "object"' > /dev/null < "$INSTALLED" \
    || die "the list of installed plugins on this box is not a shape this script knows how to read; not touching it"
  local written
  written="$(jq -r '.version // empty' "$INSTALLED")"
  [ "$written" = "2" ] \
    || die "the list of installed plugins on this box is written in a way this script does not know (it says '$written'); not touching it"
}
entry_field() {
  [ -f "$INSTALLED" ] || { printf '\n'; return 0; }
  jq -r --arg k "$SPEC" --arg f "$1" '[.plugins[$k][]? | select(.scope == "user")] | if length == 1 then .[0][$f] // "" else "" end' "$INSTALLED"
}
user_entries() {
  [ -f "$INSTALLED" ] || { printf '0\n'; return 0; }
  jq -r --arg k "$SPEC" '[.plugins[$k][]? | select(.scope == "user")] | length' "$INSTALLED"
}
check_registry

HAVE="$(user_entries)"
[ "$HAVE" -le 1 ] \
  || die "this box lists two copies or more of $PLUGIN_NAME under one name ($HAVE of them); a person has to look at that before anything else is installed"
INSTALLED_VERSION=""
if [ "$HAVE" = "1" ]; then INSTALLED_VERSION="$(entry_field version)"; fi

# `--scope user` although that is the default: everything this script READS out of the registry is
# filtered to the user scope, and a reader and a writer that agree only by default are a reader and
# a writer that can drift without a word.
say
if [ -z "$INSTALLED_VERSION" ]; then
  say "Installing $PLUGIN_NAME $VERSION…"
  run_claude "installing the plugin" plugin install "$SPEC" --scope user
elif [ "$INSTALLED_VERSION" != "$VERSION" ]; then
  say "This box has $PLUGIN_NAME $INSTALLED_VERSION and this repo is $VERSION; replacing it…"
  run_claude "updating the plugin" plugin update "$SPEC" --scope user
else
  # Same version on both sides. If the bytes also match there is nothing to do. If they do NOT
  # match, re-installing would change nothing at all: the cache directory is named after the
  # version, and a version already present is a no-op. That is the exact way a week-old bridge
  # stayed on this box for a week. So it refuses and asks for the one thing that would work.
  WAS="$(entry_field installPath)"
  if [ -n "$WAS" ] && [ -d "$WAS" ] && fingerprint "$WAS" | diff -q - "$WORK/source.sha" > /dev/null; then
    say "This box already has $PLUGIN_NAME $VERSION, byte for byte. Nothing to install."
  else
    say "This box has $PLUGIN_NAME $VERSION already, but it is not the copy in this repo:"
    if [ -n "$WAS" ] && [ -d "$WAS" ]; then
      fingerprint "$WAS" > "$WORK/here.sha"
      show_difference "$WORK/here.sha" "$WORK/source.sha"
    else
      say "  the directory it was installed into is gone"
    fi
    say
    say "  Installing $VERSION again would change nothing — the copy on this box is stored under its"
    say "  version number, so a version that is already here is never fetched again."
    say
    die "bump the version in $PLUGIN_REL/.claude-plugin/plugin.json and $PLUGIN_REL/package.json, commit, and run this again"
  fi
fi

# ── 7. verify what is actually on the box ────────────────────────────────────────────────────────
# Everything above this line is what we asked for. This is what happened.
check_registry
HAVE="$(user_entries)"
if [ "$HAVE" = "0" ]; then
  die "the install reported success but this box still lists no copy of $PLUGIN_NAME"
elif [ "$HAVE" != "1" ]; then
  die "this box now lists two copies or more of $PLUGIN_NAME under one name ($HAVE of them), so there is no single copy to check"
fi
GOT_VERSION="$(entry_field version)"
GOT_PATH="$(entry_field installPath)"
[ "$GOT_VERSION" = "$VERSION" ] \
  || die "this box now lists $PLUGIN_NAME $GOT_VERSION and this repo is $VERSION; the install did not land"
[ -n "$GOT_PATH" ] && [ -d "$GOT_PATH" ] \
  || die "this box says $PLUGIN_NAME is installed at $GOT_PATH, and there is nothing there"
[ "$(basename "$GOT_PATH")" = "$VERSION" ] \
  || die "the copy on this box is stored under $(basename "$GOT_PATH") but calls itself $VERSION; the two must agree or the next install lands in the wrong place"

fingerprint "$GOT_PATH" > "$WORK/installed.sha"
if ! diff -q "$WORK/source.sha" "$WORK/installed.sha" > /dev/null; then
  say
  say "The copy on this box is not the copy in this repo:"
  show_difference "$WORK/installed.sha" "$WORK/source.sha"
  die "the install did not produce the files this repo holds; nothing on this box can be trusted to be this bridge"
fi

# Everything above compared the files this repo tracks. This is what is on the box that it does not.
# The walk is checked rather than assumed: a listing that failed would leave an empty file, and an
# empty file here reads as "nothing unaccounted for", which is the one answer it must never give by
# accident.
if ! untracked_on_the_box "$GOT_PATH" > "$WORK/untracked"; then
  die "I could not list what is in $GOT_PATH, so I cannot say the copy there is only this bridge"
fi
if [ -s "$WORK/untracked" ]; then
  say
  say "The copy on this box holds files this repo has never seen:"
  while IFS= read -r line || [ -n "$line" ]; do printf '    %s\n' "$line"; done < "$WORK/untracked"
  say
  die "a bridge with files in it that this repo cannot account for is not this bridge"
fi

# The box records which commit it took the plugin from. It costs nothing to read, and it catches a
# copy taken from a different checkout of the same repo before a single byte is hashed.
BOX_COMMIT="$(entry_field gitCommitSha)"
HEAD_COMMIT="$(git -C "$REPO" rev-parse HEAD)"

say
say "verified:"
say "  plugin       $PLUGIN_NAME $VERSION"
say "  from         $REPO at $COMMIT"
say "  contents     $CONTENTS — the same twelve characters on two boxes means the same bridge"
say "  installed to $GOT_PATH"
say "  files        every one of the $FILE_COUNT files this repo tracks, byte for byte:"
while IFS= read -r line || [ -n "$line" ]; do
  printf '    %s  %s\n' "$(printf '%s' "$line" | cut -c1-12)" "$(printf '%s' "$line" | sed 's/^[^ ]*  //')"
done < "$WORK/installed.sha"
say "  not compared node_modules/, which is copied to the box and which this repo does not track."
say "               bun.lock is tracked and IS compared, and it is what says what belongs in there."
if [ -n "$BOX_COMMIT" ] && [ "$BOX_COMMIT" != "$HEAD_COMMIT" ]; then
  say "  note         the box says it took this plugin from commit $BOX_COMMIT, which is not the one"
  say "               checked out here. Every tracked file matches anyway, so the two commits hold"
  say "               the same bridge."
fi
say
say "  Sessions run $PLUGIN — Claude Code runs a directory marketplace's plugin from its"
say "  source, and the copy above is what \`claude plugin list\` reports. The session you are in"
say "  keeps the bridge it started with until it is restarted."

# Write it down where something other than this terminal can read it tomorrow.
mkdir -p "$RECORD_DIR"
{
  printf 'plugin       %s\n' "$PLUGIN_NAME"
  printf 'version      %s\n' "$VERSION"
  printf 'from         %s at %s\n' "$REPO" "$COMMIT"
  printf 'contents     %s\n' "$CONTENTS"
  printf 'installed to %s\n' "$GOT_PATH"
  printf 'installed at %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf 'files\n'
  cat "$WORK/installed.sha"
} > "$RECORD"
say
say "  Written down at $RECORD."

# ── 8. the door, and the two things only a person can do ─────────────────────────────────────────
say
# The value, not the word. This used to search the file for `channelsEnabled`, which a `false`
# contains as happily as a `true` — so a box with channels switched OFF was told the door was open,
# and every session started after that was deaf for a reason the install had just called fine.
if [ ! -f "$MANAGED" ]; then
  DOOR=shut
elif [ ! -r "$MANAGED" ]; then
  die "$MANAGED exists but this account cannot read it, so I cannot tell you whether the door is open"
elif ! jq -e 'type == "object"' > /dev/null < "$MANAGED"; then
  die "$MANAGED is not a shape this script knows how to read, so I cannot tell you whether the door is open"
elif jq -e '.channelsEnabled == true' > /dev/null < "$MANAGED"; then
  DOOR=open
else
  DOOR=shut
fi

if [ "$DOOR" = open ]; then
  say "The door is open: $MANAGED already enables channels."
else
  say "THE DOOR IS STILL SHUT, and only root can open it."
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
say "    echo 'CHANNEL_SPEC=plugin:$SPEC' >> <repo>/.kickoff/instance.env"

# The two halves ship from one repo and are installed by two different commands, and this one only
# ever touches the bridge. A bridge that knows about worktrees against a hub that does not is the
# ordinary intermediate state of an upgrade — the bridge now refuses rather than quietly taking the
# whole project's place, but a refusal is still a session that cannot reach him.
say
say "And restart the hub, or a session in a worktree will refuse to connect:"
say "    systemctl --user restart herdr-tg"
