<!-- MEASUREMENT, 3 September 2026. The probe docs/HUB-DESIGN.md §12 specified and nobody ran, run
     at last against the real Bot API and a real forum. Numbers here are observed, not quoted. -->

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

## What is still not measured

* Whether two *different bots* in one group have separate budgets. Every limit above is phrased as a
  property of a bot, and Telegram enforces per token, so per-bot is the strong reading — but it is a
  reading, and horizontal scaling would be built on it. It needs a second bot token to settle.
* Whether `answerCallbackQuery` or `createForumTopic` are charged. Topic creation was routed through
  the budget on 2 September on the assumption that it is.
* Anything about the paid broadcast tier.

## Running it again

The two probes live in the session scratchpad rather than in the repo, because they need the live
bot token and they put forty messages into a real forum. Re-derive them from this file if needed:
forty sends across four topics for the first, five sends and thirty edits for the second. Aim both
at throwaway topics — never at a project topic someone is reading.
