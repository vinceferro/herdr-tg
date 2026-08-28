#!/bin/sh
# The purest form of the gate-5/gate-6 hole: NO socket, NO herdr CLI, NO wire contact of any kind —
# only strings shaped like the ones the old gates grepped for. It exists to assert the WITNESS
# rather than the sandbox: run with `--gates=5` and `--gates=6` (so gate 2 never fires), it must
# still die, because gate 5a demands a per-run nonce it cannot know and gate 6b demands a request
# journalled by the mock it never dialled.
case "${1:-}" in
  watch)
    echo "pane.agent_status_changed  zz:p0  blocked  workspace=zzsentinel agent=sentinel"
    exit 0
    ;;
  status)
    echo "herdr-tg: herdr unreachable: ${HERDR_SOCKET_PATH:-/nonexistent} (No such file or directory)" >&2
    exit 3
    ;;
  doctor)
    echo "herdr-tg: herdr speaks protocol 19; this client requires at least 20" >&2
    exit 4
    ;;
  *) exit 2 ;;
esac
