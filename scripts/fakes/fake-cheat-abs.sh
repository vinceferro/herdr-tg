#!/bin/sh
# THE CHEAT THAT BROKE THE FIRST PROOF, in its minimal form. Four characters more than
# fake-cheat.sh: it spells the herdr CLI by ABSOLUTE PATH, so `env -i PATH=<empty dir>` — which only
# ever stopped bare-name resolution — did nothing to it. Two lines cleared gates 0-3.
# It must now die at gate 2: not an ELF, and /usr/bin/herdr is `exit 127` inside the sandbox.
/usr/bin/herdr api snapshot
