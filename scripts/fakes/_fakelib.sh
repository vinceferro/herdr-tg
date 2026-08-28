# Sourced by every fake EXCEPT fake-cheat.sh (which is the cheat, and must stay two lines).
#
# WHY THE ABSOLUTE PATH: proof-slice1.sh runs the client as
#   env -i HOME="$HOME" PATH="$(mktemp -d)" "$BIN" ...
# so nothing on the operator's PATH resolves. A real Rust client dials the socket with libc and
# needs no PATH at all; these stand-ins need jq and socat, so they name the directories that hold
# them. That is NOT the cheat gate 2 catches — gate 2 catches a client that resolves the *herdr
# CLI* through PATH, which is exactly what fake-cheat.sh does and why it exits 127 there.
PATH=/usr/bin:/bin
export PATH

# Same fallback the real client implements: $HERDR_SOCKET_PATH, else ~/.config/herdr/herdr.sock.
# Under the gate-2/3 sandbox every HERDR_* var is stripped, so this takes the fallback branch.
SOCK="${HERDR_SOCKET_PATH:-$HOME/.config/herdr/herdr.sock}"

# One-shot RPC, exactly like the real server: one request per connection, connection closes.
rpc(){ printf '%s\n' "$1" | socat -t 5 - "UNIX-CONNECT:$SOCK" 2>/dev/null; }

# The full {"id","result":{"type":"session_snapshot","snapshot":{…}}} envelope, straight off the
# socket — normalize.jq begins at `.result.snapshot`, so the envelope is mandatory.
snapshot_envelope(){ rpc '{"id":"fake","method":"session.snapshot","params":{}}'; }

# Every fake implements only `status --json`; proof-slice1.sh --gates=3 calls nothing else.
require_status(){
  case "${1:-}" in
    status) : ;;
    *) printf 'fake client: only `status --json` is implemented (got: %s)\n' "${1:-<none>}" >&2; exit 2 ;;
  esac
}

# Emit a mutated envelope. $1 = a jq program applied to the whole envelope.
emit(){ snapshot_envelope | jq -c "$1"; }
