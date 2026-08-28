#!/bin/sh
# Forges gate 6b's contact witness instead of dialling the mock.
#
# Round-two review found gate 6b asserted contact by grepping a journal that sat beside the witness
# socket — a path this script can derive from HERDR_SOCKET_PATH. Appending the expected line was
# enough to pass, and the gate then printed "mock journalled the ping" about a mock that was never
# opened. The journal now lives in an unguessable directory the client is never given, so the
# derivation below finds nothing and gate 6b fails. This fake exists so that regression cannot
# return silently: if it ever passes again, the witness has moved back within the client's reach.
case "${1:-}" in
  status)
    # Gate 6a runs first and must be satisfied, or this fake dies before it reaches the gate it
    # exists to probe. Same canned answer fake-cheat-canned.sh gives.
    echo "herdr-tg: herdr unreachable: ${HERDR_SOCKET_PATH:-/nonexistent} (No such file or directory)" >&2
    exit 3
    ;;
  doctor)
    d="${HERDR_SOCKET_PATH%/*}"
    # Every path the old implementation used, plus the obvious neighbours.
    for j in "$d/skew.journal" "$d/ev.journal" "$d"/*.journal; do
      printf '{"method": "ping", "n": 0, "params": null}\n' >>"$j" 2>/dev/null || true
    done
    echo "herdr-tg: herdr speaks protocol 19; this client requires at least 20" >&2
    exit 4
    ;;
  *) exit 2 ;;
esac
