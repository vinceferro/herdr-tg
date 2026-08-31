---
name: opencode-has-a-public-server-api
description: opencode ships a headless HTTP server with an OpenAPI spec and an event stream, so a bridge attaches to a public surface — no plugin, no patching, and independent of kickoff's opencode telegram patch
metadata:
  type: reference
---

Measured on opencode 1.18.25, 2026-08-31:

```
$ opencode serve --port <p> --hostname 127.0.0.1
opencode server listening on http://127.0.0.1:<p>
$ curl -s http://127.0.0.1:<p>/doc     -> {"openapi":"3.1.0","info":{"title":"opencode", ...}}
$ curl -sN http://127.0.0.1:<p>/event  -> held open (a stream, not a 404)
```

Also `opencode attach <url>` and `opencode acp` (Agent Client Protocol server).

**Why this matters and why it is easy to get wrong.** kickoff's `patches/opencode-telegram-bot-v6`
reaches opencode's events through `../../opencode/events.js` — an INTERNAL module, inside opencode's
own tree, and not present in this box's install at all. Building on that name would make us
downstream of their patch. The public server API is the right surface, and it means whether that
patch works on a given box is irrelevant to us.

**The asymmetry worth remembering:** Claude Code has no external API for an agent's own turns, so a
channel plugin is the only door there. opencode has a documented one. The engine with the weaker
surface is the one that needs the plugin.

**Security note:** `opencode serve` warns `OPENCODE_SERVER_PASSWORD is not set; server is unsecured`.
A local control-plane port with no auth, on a box running several orgs' agents. Set it before
anything attaches. See [[herdr-reports-no-structured-ask]] for the contrast — the whole reason a
structured surface matters.
