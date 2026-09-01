# kickoff-channel

The agent's half of the herdr-tg hub. Claude Code starts it as this session's channel; it dials
`/run/user/<uid>/kickoff/hub.sock` and speaks the frames in `crates/hub-proto`.

It holds **no** Telegram token, no chat allowlist and no LLM. The hub owns all three. This process
knows one project — its own — and cannot name another: the secret at `.kickoff/hub.token` is what
the hub resolves, and the `project_id` it sends is not consulted.

## What the agent gets

| tool | what it does |
| --- | --- |
| `reply(text)` | says something to the operator. Does not buzz his phone. |
| `ask(text, options)` | asks a question. Buzzes. The answer arrives as a channel message, not as this tool's return value — the operator may take hours, and a tool call that blocked for hours would be a session that looked hung. |
| `done(text)` | the turn finished. Buzzes. |

Answers and typed messages arrive as `notifications/claude/channel`, which Claude Code injects into
the agent's own turn. That is the whole safety story of the redesign: **a reply is a message in the
agent's turn, never a keystroke in its terminal**, so the operator's phone and his laptop are no
longer two writers fighting over one keyboard.

## Enrolling

    herdr-tg enroll <repo>      # mints <repo>/.kickoff/hub.token, mode 0600

`hub.token` is this project's secret. Make sure git ignores it — `enroll` asks git and warns if not.

## When the hub is not there

The socket is absent whenever the hub is not running, which is the ordinary state on a box where it
has not been started. The plugin says so once, keeps retrying on a widening backoff, and never
blocks the agent's turn: a channel that hangs is worse than one that is plainly down.
