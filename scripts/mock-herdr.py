#!/usr/bin/env python3
"""A scripted stand-in for herdr on a UNIX socket — the proof's INDEPENDENT WITNESS.

Two jobs, and the second one is the point:

1. `--protocol N` answers `ping` with a chosen protocol, then closes the connection (one request
   per connection, exactly like the real server). Proof gate 6b drives the protocol-19 skew path
   with it.

2. `--events` additionally serves `events.subscribe`: it acks with `subscription_started`, then
   writes TWO frames — a snake_case `pane_updated` roster frame carrying a historical
   `agent_status` (the decoy the two-envelope decoder must not mistake for an edge) and the
   dot-form `pane.agent_status_changed` frame the product exists for. The dot-form frame carries
   SENTINEL ids and a per-run nonce that appear nowhere in the live herd, so a client that prints a
   canned string prints the wrong sentinel, and a client that cannot decode has nothing to print.

`--journal FILE` appends one JSON line per received request, flushed immediately. That is the other
half of the witness: it lets a gate assert THE MOCK WAS ACTUALLY CONTACTED and that the request was
correctly shaped, which no amount of fabricated client stdout can satisfy.

Read-only and self-contained: it binds a socket the caller names (a mktemp dir, never the
operator's), speaks to nothing else, and never touches the live herd.
"""

import argparse
import json
import os
import socket
import sys

ap = argparse.ArgumentParser()
ap.add_argument("--socket", required=True)
ap.add_argument("--protocol", type=int, default=19)
ap.add_argument("--version", default="0.7.9")
ap.add_argument("--requests", type=int, default=4)
ap.add_argument("--journal", default=None, help="append one JSON line per request received")
ap.add_argument("--events", action="store_true", help="also serve events.subscribe (gate 5a)")
ap.add_argument("--sentinel-pane", default="zz:p0")
ap.add_argument("--sentinel-workspace", default="zzsentinel")
ap.add_argument("--sentinel-status", default="blocked")
ap.add_argument("--sentinel-agent", default="sentinel",
                help="the per-run nonce; `-` reads one line from STDIN and then closes it")
a = ap.parse_args()

# The nonce must not be discoverable by the very client being tested. `unshare -rm` is a USER +
# MOUNT namespace, not a PID namespace, so /proc is the host's and every `/proc/<pid>/cmdline` is
# world-readable — a nonce passed on the command line can be scraped and echoed back without ever
# opening a socket, which was demonstrated against an earlier revision of this file. So the gate
# feeds it on STDIN and fd 0 is re-pointed at /dev/null the moment it has been read, leaving the
# value only in this process's memory.
if a.sentinel_agent == "-":
    a.sentinel_agent = sys.stdin.readline().strip()
    if not a.sentinel_agent:
        raise SystemExit("mock-herdr.py: --sentinel-agent - was given but stdin carried no nonce")
    devnull = os.open(os.devnull, os.O_RDONLY)
    os.dup2(devnull, 0)
    os.close(devnull)

if os.path.exists(a.socket):
    os.unlink(a.socket)
srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(a.socket)
os.chmod(a.socket, 0o600)
srv.listen(8)

journal = open(a.journal, "a", buffering=1) if a.journal else None


def record(**fields):
    if journal:
        journal.write(json.dumps(fields, sort_keys=True) + "\n")
        journal.flush()
        os.fsync(journal.fileno())


def send(conn, obj):
    conn.sendall((json.dumps(obj) + "\n").encode())


# The decoy: the snake_case roster family. It carries a HISTORICAL agent_status equal to the
# sentinel status, which is exactly the trap — `pane.updated` replays an ageing backlog on every
# connect, so a client that read status from this family would fire a phantom edge. The decoded
# RosterEvent structurally cannot carry a status, and `watch --once` must skip this frame and wait
# for the dot-form one. Sending it first is what makes gate 5a prove the DISCRIMINATION and not
# merely "some frame arrived".
def roster_frame():
    return {
        "event": "pane_updated",
        "data": {
            "type": "pane_updated",
            "pane": {
                "pane_id": a.sentinel_pane,
                "terminal_id": "t-sentinel",
                "workspace_id": a.sentinel_workspace,
                "tab_id": "zz:t0",
                "focused": False,
                "agent_status": a.sentinel_status,
                "revision": 7,
            },
        },
    }


# The one the product exists for: dot-form, and `data` carries NO `type` key at all.
def sentinel_frame():
    return {
        "event": "pane.agent_status_changed",
        "data": {
            "pane_id": a.sentinel_pane,
            "workspace_id": a.sentinel_workspace,
            "agent_status": a.sentinel_status,
            "agent": a.sentinel_agent,
        },
    }


print("ready", flush=True)
served = 0
try:
    for _ in range(a.requests):
        conn, _addr = srv.accept()
        line = b""
        while not line.endswith(b"\n"):
            chunk = conn.recv(65536)
            if not chunk:
                break
            line += chunk
        try:
            req = json.loads(line.decode())
        except Exception:
            req = {}
        rid = req.get("id", "")
        method = req.get("method", "")
        record(n=served, method=method, params=req.get("params"))
        served += 1

        if a.events and method == "events.subscribe":
            send(conn, {"id": rid, "result": {"type": "subscription_started"}})
            send(conn, roster_frame())
            send(conn, sentinel_frame())
            # Half-close: the frames are already in the socket buffer, so the client reads both and
            # then sees a clean EOF. `watch --once` returns on the first MATCHING frame.
            try:
                conn.shutdown(socket.SHUT_WR)
            except OSError:
                pass
        else:
            send(conn, {"id": rid, "result": {
                "type": "pong", "version": a.version, "protocol": a.protocol,
                "capabilities": {"live_handoff": False, "detached_server_daemon": False}}})
        conn.close()
finally:
    record(n=served, method="__closed__", params=None)
    srv.close()
    try:
        os.unlink(a.socket)
    except OSError:
        pass
    if journal:
        journal.close()
