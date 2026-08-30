#!/usr/bin/env bash
# eval-gist.sh — score the gist prompt against a fixed set of asks, and print the table.
#
#   bash scripts/eval-gist.sh                 # the shipped prompt, via the configured class
#   bash scripts/eval-gist.sh --compare       # local vs hosted, side by side
#   bash scripts/eval-gist.sh --prompt FILE   # score a candidate prompt before adopting it
#   bash scripts/eval-gist.sh --class bulk    # force a routing class
#
# WHY THIS EXISTS
#
# The claim "8/8 at 390ms, matching a hosted model at a fifth of the latency" is only worth
# something if you can re-run it. The prompt in prompts/gist.txt was written by a strong model FOR
# a small one, and that kind of prompt is brittle in ways ordinary code is not: a model update, a
# routing change, or an innocent-looking edit can quietly halve the score. This turns the claim into
# a command.
#
# WHAT IT MEASURES, AND WHAT IT DOES NOT
#
# It scores the distribution that actually occurs: panes herdr has ALREADY flagged as blocked. The
# gist is never called on anything else, so a mixed set that includes idle panes measures the wrong
# thing — that mistake nearly lost the better prompt during distillation.
#
# The grader is mechanical, not semantic. It checks the reply is a single short line, that it ends
# up asking rather than answering, and that it did not pick one of the offered options. It cannot
# tell you the restatement is FAITHFUL — read the table for that. A high score with wrong wording is
# possible and the output is printed so you can see it.
#
# It needs the gateway running and HERDR_TG_SUMMARIZER_KEY set; without them it says so and exits 2
# rather than reporting a zero that looks like a regression.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROMPT="$REPO/crates/herdr-tg/prompts/gist.txt"
CASES="$REPO/crates/herdr-tg/tests/fixtures/gist-cases.json"
ENVF="${HERDR_TG_ENV:-$HOME/.config/herdr-tg/env}"
CLASS=""
COMPARE=0

while [ $# -gt 0 ]; do
  case "$1" in
    --compare) COMPARE=1 ;;
    --prompt)  PROMPT="${2:?--prompt needs a file}"; shift ;;
    --class)   CLASS="${2:?--class needs a value}"; shift ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

[ -f "$PROMPT" ] || { printf 'no prompt at %s\n' "$PROMPT" >&2; exit 2; }
[ -f "$CASES" ]  || { printf 'no cases at %s\n' "$CASES" >&2; exit 2; }

if [ -z "${HERDR_TG_SUMMARIZER_KEY:-}" ] && [ -f "$ENVF" ]; then
  # shellcheck disable=SC1090
  set -a; . "$ENVF"; set +a
fi
[ -n "${HERDR_TG_SUMMARIZER_KEY:-}" ] || {
  printf 'HERDR_TG_SUMMARIZER_KEY is not set and %s does not carry it.\n' "$ENVF" >&2
  printf 'Nothing was measured — this is not a score of zero.\n' >&2
  exit 2
}
URL="${HERDR_TG_SUMMARIZER_URL:-http://127.0.0.1:8090/v1/chat/completions}"
[ -z "$CLASS" ] && CLASS="${HERDR_TG_SUMMARIZER_CLASS:-autocomplete}"

export PROMPT CASES URL CLASS COMPARE
python3 - <<'PY'
import json, os, statistics, sys, time, urllib.error, urllib.request

PROMPT = open(os.environ["PROMPT"], encoding="utf-8").read()
CASES = json.load(open(os.environ["CASES"], encoding="utf-8"))
URL, KEY = os.environ["URL"], os.environ["HERDR_TG_SUMMARIZER_KEY"]

def ask(excerpt, cls, model=None, timeout=90):
    body = {"max_tokens": 500, "temperature": 0,
            "messages": [{"role": "system", "content": PROMPT},
                         {"role": "user", "content": excerpt}]}
    if model:
        body["model"] = model
    headers = {"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"}
    if cls:
        headers["X-Task-Class"] = cls
    req = urllib.request.Request(URL, json.dumps(body).encode(), headers)
    t = time.time()
    try:
        r = json.load(urllib.request.urlopen(req, timeout=timeout))
        msg = r["choices"][0]["message"]
        return ((msg.get("content") or "").strip(), r.get("model", "?"),
                int((time.time() - t) * 1000), None)
    except Exception as e:                       # noqa: BLE001 - any failure is a failed case
        return ("", "?", int((time.time() - t) * 1000), str(e))

# Mirrors crates/herdr-tg/src/summarize.rs::plausible. Kept in step by hand; the Rust side is the
# authority, and a divergence shows up here as a score that disagrees with what the bot sends.
FILLER = ("please provide", "the necessary information", "no question", "not asking")
def grade(out, excerpt):
    o = out.strip().strip('"').rstrip(".")
    if not o or "\n" in o or len(o) > 110:
        return False, "not one short line"
    if o.lower() in ("none", "none."):
        return False, "said NONE to a real ask"
    if o.lower().startswith("is the agent"):
        return False, "echoed the instruction"
    if any(f in o.lower() for f in FILLER):
        return False, "filler"
    if "?" not in o:
        return False, "answered instead of restating"
    # picking an offered option verbatim is the dangerous failure
    for line in excerpt.split("\n"):
        for opt in [p.strip() for p in line.split("   ") if p.strip()]:
            if len(opt.split()) <= 3 and opt.lower() == o.lower():
                return False, f"picked the option {opt!r}"
    return True, ""

def run(label, cls, model=None):
    ok, ms, rows = 0, [], []
    for c in CASES:
        out, mdl, t, err = ask(c["excerpt"], cls, model)
        good, why = (False, err) if err else grade(out, c["excerpt"])
        ok += good; ms.append(t)
        rows.append((c["id"], good, why, out, mdl))
    med = statistics.median(ms) if ms else 0
    served = rows[0][4] if rows else "?"
    print(f"\n  {label}")
    print(f"  {'':2}{ok}/{len(CASES)}   median {int(med)}ms   served by {served}")
    for cid, good, why, out, _ in rows:
        mark = "\033[32m✓\033[0m" if good else "\033[31m✗\033[0m"
        note = "" if good else f"   \033[33m← {why}\033[0m"
        print(f"    {mark} {cid:14} {out[:58]!r}{note}")
    return ok, len(CASES)

print(f"gist evaluation · {len(CASES)} blocked-pane asks")
print(f"prompt: {os.environ['PROMPT']}")

if os.environ["COMPARE"] == "1":
    a = run(f"local  · X-Task-Class: {os.environ['CLASS']}", os.environ["CLASS"])
    b = run("hosted · no class (default chain)", None)
    print(f"\n  local {a[0]}/{a[1]} · hosted {b[0]}/{b[1]}")
    sys.exit(0 if a[0] == a[1] else 1)
else:
    ok, n = run(f"X-Task-Class: {os.environ['CLASS']}", os.environ["CLASS"])
    print()
    if ok == n:
        print(f"  GIST EVAL: PASS — {ok}/{n}")
        sys.exit(0)
    print(f"  GIST EVAL: FAIL — {ok}/{n}. The prompt regressed, or the routing moved.")
    sys.exit(1)
PY
