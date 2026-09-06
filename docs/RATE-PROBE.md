<!-- MEASUREMENT, 3 September 2026; §3 added 5 September; two owed measurements about uploads
     added 6 September with files (docs/ATTACHING.md §14). The probe docs/HUB-DESIGN.md §12
     specified and nobody ran, run at last against the real Bot API and a real forum. Numbers here
     are observed, not quoted. What is listed as NOT measured is not measured: nothing below the
     fold has been quietly assumed into a number above it. -->

# What the Bot API actually charges

Two questions the whole pacing design rests on, both open until today. `queue.rs` claimed the first
had been measured and cited §12 for it; §12 describes the probe in the future tense and no result
was ever recorded. The answer happens to match the assumption, which is why nobody noticed.

## 1. The 20/min group ceiling is per CHAT, not per topic

Forty sends, spread round-robin across **four different topics** of one forum, as fast as they would
go:

```
attempted=40  accepted=20  seconds=21
first 429 at send 21, retry_after=41
```

Spreading across topics bought nothing. Twenty went, the twenty-first was refused, and every
subsequent attempt in that minute was refused too — in whichever topic it was aimed at. So topics
are threads inside one chat, and the budget belongs to the chat.

**It is a window, not a rate.** Twenty were accepted in the first 19 seconds with no pacing at all,
and `retry_after` then counted down the remainder of the minute (41, 40, 39 …).

> This paragraph used to call a token bucket "the conservative shape for that … it will never exceed
> the ceiling". That was wrong, and it is the sentence that stopped anyone looking. A bucket of
> capacity `C` refilling at `r` allows `C + 60r` in any sixty seconds, which at the shipped constants
> was `18 + 18 = 36` — measured on the code itself at **34**, against a ceiling of twenty. A bucket
> bounds the sustained rate; Telegram enforces a window. `queue.rs` counts the sends inside the
> trailing minute now, which is what was measured rather than a model of it.

Note also what `retry_after` *is*: the moment the oldest send in Telegram's own window ages out of
it. So a chat coming back from a flood wait has room for one send, not for a whole fresh burst.

The Bot FAQ agrees, and phrases both limits as properties of a bot rather than of a chat:

> In a single chat, avoid sending more than one message per second.
> In a group, bots are not be able to send more than 20 messages per minute.

Note the difference in force — the per-second one is advice, the per-minute one is enforced.

## 2. `editMessageText` is NOT charged against it

Five sends to seed message ids, leaving fifteen of the twenty unspent, then thirty edits:

```
edits: accepted=30  refused429=0  other=0
send afterwards: OK — edits did not spend the send budget
```

Thirty edits, no refusal, and a send still went through. **An edit is free against this ceiling.**

Two consequences:

* Taking a keyboard off a resolved question costs nothing. `docs/MULTIPLEXER-READINESS.md` §3 grades
  the unmetered retirement edit at `hub.rs:1478` as a scaling risk on the assumption it spends from
  the same budget. It does not, and that row should be re-graded rather than fixed.
* **Editing in place is the cheap way to say something twice.** A conversation that updates one
  message costs one token; one that posts each update costs one per update. At the ceiling this is
  the difference between a status line that keeps up and one that is shed.

## 3. `setMessageReaction` is NOT charged against it — and has a ceiling of its own

Measured 5 September, in a throwaway topic. Five seeds to have messages to react to, then a check of
which emoji the API takes at all, then thirty reactions round-robin over the seeds — each one
*replacing* the last, which is how the hub will use them — then one send:

```
seeds: 5 sent in 0.6s
emoji ✅: HTTP 400 Bad Request: REACTION_INVALID
emoji ❌: HTTP 400 Bad Request: REACTION_INVALID
emoji 👀: HTTP 200 ok
emoji 👍: HTTP 200 ok
emoji 👎: HTTP 200 ok
  reaction 18: 429 retry_after=35
  … (every one after it, retry_after counting down)
reactions: accepted=17 refused429=13 other=0 seconds=24.9
send afterwards: OK — reactions did not spend the send budget
clear (empty list): HTTP 429 Too Many Requests: retry after 34
```

Three findings, and the third is the one nobody asked about:

* **A reaction does not spend a send.** Five sends, twenty accepted reactions, and a send still went
  through with the reaction bucket empty. Marking the operator's own message is free against the
  ceiling that rations the agents.
* **The tick and the cross do not exist.** `✅` and `❌` are refused outright: a bot's free reactions
  are Telegram's fixed list, and neither is on it. `👀` is; `👍` and `👎` are the nearest honest pair
  for "the agent has it" and "it did not reach the agent". The hub uses those three.
* **Reactions have a ceiling of their own: twenty in a trailing minute, the same shape as sends,
  counted separately.** Three accepted in the emoji check plus seventeen in the run is twenty, and
  the twenty-first was refused with a `retry_after` that counted down the rest of the minute — and
  clearing a reaction is a reaction call, so it was refused too. A first run of this probe, smeared
  over three minutes by a slow HTTP client, hit the same wall after the same twenty and then had
  reactions accepted again as the oldest aged out.

What follows for the hub: a reaction goes through **no** send accounting, and it has a ledger of
its own — eighteen of the measured twenty in a trailing minute, under the ceiling by the same two
as sends and for the same reason — past which the hub stops asking rather than walking every later
mark into a `429`. It is never retried and never waited for: a mark the ledger or Telegram refuses
is a mark that does not appear, the line in the topic still carries the meaning, and nothing an
agent is waiting on is behind it. One of his messages costs at most two reactions (the eyes, then
the thumb up or down), so the wall is nine of his lines in one minute, which a thumb does not reach.

Not measured here: a reaction on a message HE sent rather than one the bot sent. Reacting to other
people's messages is the ordinary use of the call and there is no reason to expect a difference,
but no message of his was in a throwaway topic to try it on.

## What is still not measured

* Whether two *different bots* in one group have separate budgets. Every limit above is phrased as a
  property of a bot, and Telegram enforces per token, so per-bot is the strong reading — but it is a
  reading, and horizontal scaling would be built on it. It needs a second bot token to settle.
* Whether `answerCallbackQuery` or `createForumTopic` are charged. Topic creation was routed through
  the budget on 2 September on the assumption that it is.
* Whether the reaction ceiling is per chat, like the send ceiling, or per bot.
* Whether `sendPhoto` and `sendDocument` are charged against the 20/min ceiling like a text.
  `docs/ATTACHING.md` §14.4 assumes they are and takes a turn from the conversation's budget for
  every upload, which is the assumption that fails closed: an upload that took no turn would be
  paid for by whichever project sent next. Settling it costs one throwaway topic and a handful of
  small pictures.
* Whether a real full-page screenshot is refused by `sendPhoto` for its dimensions or merely
  downscaled. The hub already refuses to guess — an agent that wants him to READ a page says
  `as: "document"` — but which of the two Telegram does decides whether the sentence he gets back
  ("ask for it as a document") is ever the right advice.
* Anything about the paid broadcast tier.

## Running it again

The three probes live in the session scratchpad rather than in the repo, because they need the live
bot token and they put messages into a real forum. Re-derive them from this file if needed: forty
sends across four topics for the first, five sends and thirty edits for the second, five sends and
thirty reactions then one send for the third. Aim all of them at throwaway topics — never at a
project topic someone is reading — and never call `getUpdates`, which the running hub holds. Use a
client that answers in a fraction of a second, or the window slides under the run.
