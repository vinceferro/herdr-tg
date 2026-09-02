# opencode-bridge

Seam ② for the other engine. One opencode server, one project, one hub connection.

```
OPENCODE_BRIDGE_REPO=/path/to/the/enrolled/repo \
OPENCODE_URL=http://127.0.0.1:9700 \
bun bridge.ts
```

Both variables have defaults: the repo is the working directory, and the server is
`http://127.0.0.1:9700`. The hub socket is derived from the uid and is never configured.

## What it maps

| opencode publishes | reaches the phone as |
| --- | --- |
| `question.v2.asked` (and the v1 form) | `ask`, with the labels opencode published as buttons |
| `permission.v2.asked` (and the v1 form) | `ask`, with opencode's own closed set: once · always · reject |
| `question.v2.replied` / `rejected`, `permission.v2.replied` | `ask_resolved` — the buttons come off |
| `session.idle` | `beat` |

A tap goes back to opencode as the **label it published**, at the endpoint the OpenAPI spec names
for that event family. The v2 endpoints live under `/api` and name the session; the v1 ones do not.
Getting that wrong posts an answer into a 404 and leaves the question open on his phone with the
agent still waiting, which is why the family each ask came from is recorded rather than guessed.

## What it refuses to do

- **It never uses `/session`'s `directory` to decide which project a session belongs to.** Identity
  is a secret the hub resolves. One bridge process per project, addressed by URL — which is also
  what survives each agent moving into its own container.
- **It never invents a choice.** Every button comes from a list opencode published, and a tap is
  looked up in the record this bridge wrote down. An option nobody minted answers nothing.
- **It does not pass typed steering to opencode yet.** The hub relays what the operator types, and
  this bridge says so and stops. Prompting a session by text is a second decision.

## Why this is not the screen scraper

The pane-reading design died because herdr's protocol carries agent status and raw screen bytes and
nothing about what an agent is asking, so every prompt had to be reconstructed from output that had
already thrown the structure away. opencode publishes the question, its options, and the fact that
it stopped being asked. Nothing here parses a screen.

## Test

```
bun test-against-fakes.ts
```

The real bridge, spawned as its own process, against a fake hub on a real Unix socket and a fake
opencode over real HTTP. Nothing is mocked inside the bridge: the handshake and the queue's
before-`welcome` rule are the two things that have actually broken this project, and only an
outside observer proves them.
