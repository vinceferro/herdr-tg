#!/bin/sh
# The same escape by the other door: rebuild PATH inside the sandbox, then resolve `herdr` bare.
# scripts/fakes/_fakelib.sh does exactly this line for jq and socat — legitimately, since a stand-in
# needs them and the real client needs no PATH at all — which is what made it the obvious next cheat.
PATH=/usr/bin:/bin:/usr/local/bin
export PATH
herdr api snapshot
