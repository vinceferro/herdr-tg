<!-- SUPERSEDED IN PART, 4 September 2026. The variable this proposal calls
     `KICKOFF_CHANNEL_CONVERSATION` shipped as `KICKOFF_HUB_ADDRESS`, in the one namespace
     `docs/ATTACHING.md` defines. Everything else here is still a proposal. -->
<!-- PROPOSAL, 3 September 2026. One of four competing answers to "rooms cannot connect". This is the
     CONSERVATIVE one: it moves the secret and nothing else. No production code was changed to write
     it; every behavioural claim carries a file:line or was measured on this box and says which. -->

# The conversation record

**The channel keeps the secret, the launcher names the conversation, and a repo is just a field on
the record.**

That is the whole change. The token stays a 64-hex secret compared against a stored SHA-256. The
registry stays one file. The wire does not move a byte. What moves is the *file the secret sits in*
— out of `<repo>/.kickoff/hub.token` and into the channel's own space — and, with it, the question
the bridge answers before it dials: no longer *"which directory am I in"* but *"which conversation
was I launched as"*.

Everything else in this document is consequence.

---

## 1. Why the directory has to stop being the answer

`factsFor` sets a lane only inside a linked git worktree (`plugins/kickoff-channel/where.ts:86`);
`findProject` walks up to the nearest `.kickoff/hub.token` (`where.ts:135-158`). So a session in a
repo's main tree presents `(that repo's secret, no lane)` — always, whatever it was started to do.
Three rooms of one org launched in that repo present the identical address and the second is refused
`already_claimed` at `crates/kickoff-channel/src/hub.rs:1127`. `docs/TAXONOMY.md` §1 has the full trace.

The address is a *function of the launch directory*. Two rooms in one directory is a collision that
function cannot avoid, and no amount of care in the hub can fix it, because the hub is given one
address and is right to believe it.

The operator's steer names the fix without naming the mechanism: conversations live *outside* the
repo, in a space the channel owns and lays out as it likes. If the secret is not in the tree, the
tree cannot be what selects it.

---

## 2. Where they live

Measured: `~/.claude/channels/` exists on this box and is **empty**. There is no schema to copy and
no second occupant to interoperate with, so "follow the Claude convention" would mean following a
directory name. What is worth copying is the *shape*: a channel owns a space, keyed by channel, and
arranges it as it sees fit.

`${XDG_STATE_HOME:-~/.local/state}/herdr-tg/` (`crates/kickoff-channel/src/lock.rs:39`) is already that
space. It is already outside every repo, already the channel's, already unimposed-upon. What it is
not is *scoped by conversation* — it is one flat pile of twelve files. This proposal adds exactly one
level of scoping inside it:

```
<channel space>/                     0700  ← asserted, see the hazard below
  projects.json                      0600  unchanged name, unchanged shape, one new field
  projects.lock  asks.json  hub.audit.log  hub.lock  hub.heartbeat   unchanged
  conversations/                     0700
    herdr-tg/                        0700  one directory per conversation, named by its HANDLE
      secret                         0600  the bytes that used to be <repo>/.kickoff/hub.token
    hub-dogfood/secret
    ops/secret
  default-for/                       0700
    <sha256(main worktree path)[..16]>   0600  one line: a handle
```

A **directory** per conversation rather than a bare `secret` file, for one forward-looking reason
worth one `create_dir_all`: `asks.json` is today rewritten whole on every ask and every tap
(`hub.rs:573`) and is the file this project has already measured as the one that compounds
(`registry.rs:88`'s comment). When it eventually becomes per-conversation, it lands in a folder that
already exists. Nothing moves now.

**A hazard that must be fixed first, and it is measured.** The only `set_permissions(dir, 0o700)` in
the tree is `hub.rs:813`, for the runtime dir. `lock.rs:120` and `registry.rs:553` both use bare
`create_dir_all`. Probed on this box: a fresh directory under umask 0022 comes out **0755**. This box's
own space is 0700 by history, not by code. Putting secrets under a directory that ships world-readable
on every new box is not acceptable, so step 1 of the migration is four lines asserting 0700, before
anything else.

**If he wants the literal location**, one env var (`HERDR_TG_STATE_DIR`) beside the existing
`XDG_STATE_HOME` term lets the whole space sit at `~/.claude/channels/herdr-tg/`. It is a one-line
addition to `lock.rs:39` and this proposal does not depend on it either way. Note the bridge needs
the *same* derivation, and it needs to be overridable for the same reason the socket is
(`hub/tests.rs:1598` runs the real plugin against a tempdir).

---

## 3. The record

`Project` (`registry.rs:48-89`) is the record already. It gains one field and loses nothing:

| field | today | after |
| --- | --- | --- |
| `id` | `"p-" + sha256(canonical repo)[..12]`, minted at enrol (`registry.rs:421`) | **minted once and then carried verbatim.** Never re-derived at runtime. |
| `handle` | — | **NEW.** What the operator types, what a launcher passes down, and the directory name in the channel space. Unique across the channel; `[a-z0-9][a-z0-9._-]{0,31}`. |
| `title` | repo basename, sanitised, deduped (`registry.rs:479`) | same when unnamed; **the handle** when the conversation was named. |
| `repo` | the identity — the id is a hash of it | **an attribute.** Where this conversation's work happens. May be shared with other conversations. May be absent. |
| `token_sha256` | hex SHA-256 of the secret | unchanged, and the secret is still never stored here. |
| `enabled`, `topic_id`, `icon_color`, `lane_topics` | — | untouched, byte for byte. |

There is no `kind` field. A seed, a room and a lane are not words the hub learns —
`docs/CAPABILITIES.md` REFUSES §6 is a line, and this design does not cross it. A **room is a
top-level conversation**; a **lane is still a lane of one**. That falls out for free and it is worth
naming: because a room is never addressed as a lane, the three operator-facing strings that say
"worktree" (`hub.rs:1427`, `bot.rs:500`, `bot.rs:521` — `docs/TAXONOMY.md` §3) stay *true*. This
design does not need them changed. Any design that addresses a room as a lane does.

**`handle` never goes on the wire.** It is a filesystem selector, read in the bridge's own process,
before it dials. The hub has not heard of it. That matters for §6.

`#[serde(default)]` on `handle`, exactly as `lane_topics` did (`registry.rs:88`), so a registry
written by today's build still parses on a build that has heard of handles — and the migration fills
it in for the three live rows anyway.

---

## 4. How a bridge names which conversation it is

Three terms, ordered, and the order is the safety argument. It deliberately mirrors the ladder this
repo already uses for `LAUNCHED_IN` (`server.ts:88-92`), because that ladder was written after the
defect where cwd was guessed, and its lesson is the same one: *a claim somebody made beats a guess
this file made, and not knowing fails closed.*

1. **`KICKOFF_CHANNEL_CONVERSATION`** — the handle, set by whoever dispatched the session. This is
   the room's path. Kickoff spawning a room sets it; nothing is derived.
2. **The launch directory's default.** Ask git for `--git-common-dir`'s parent — the **main** working
   tree — hash it, read `default-for/<hash>`, get a handle. This is the seed's path: the operator
   opens a session in his repo by hand and it just works, with no env var.
3. **Neither** → refuse, permanently, with a sentence naming the terminal command.

Then: read `conversations/<handle>/secret`, and present it in `hello` exactly as today.

Three things this buys, and the second is the one worth the change:

* **`where.ts:143` disappears.** The cross-boundary hop into the main worktree exists only because a
  gitignored token is never checked out into a linked worktree. Keying term 2 on the *main* tree
  makes a lane worktree and its main tree read the same conversation by construction. One whole class
  of "this project is not enrolled" is gone rather than handled.
* **`server.ts:493` becomes unwritable.** The enrol hint that today names the *worktree*, and whose
  advice mints a second project (`docs/TAXONOMY.md` §6), has no repo path to print any more.
* **A container needs one bind-mount, not a repo.** `conversations/<handle>/secret`, one file,
  read-only, at the same path inside. Today the equivalent is bind-mounting a directory inside the
  org's working tree.

**What term 1 is, precisely.** A selector, not a credential. Naming a handle tells the bridge which
file to *attempt to read*; reading it requires being this uid — which, measured, is the same
authority that could already read every project's `.kickoff/hub.token` and every other process's
`/proc/<pid>/environ`. The trust boundary does not move. Nothing is granted by naming.

---

## 5. What `enroll` becomes

One command, one new flag.

```
herdr-tg enroll <folder>                 # this folder's default conversation — the seed
herdr-tg enroll <folder> --as ops        # a second conversation whose work happens there — a room
herdr-tg enroll --as scratch             # a conversation with no folder at all
```

* **Unnamed**: id is `"p-" + sha256(canonical folder)[..12]` — **byte-identical to
  `registry.rs:421`.** Writes `default-for/<hash of the main tree>` → handle. Title from the basename,
  as today.
* **Named**: id is 12 random hex with the same `p-` shape, minted once and carried. Writes **no**
  pointer — a room is nobody's default. Title is the handle. The `InsideAnotherProject` guard
  (`registry.rs:406-412`) does not apply: two conversations sharing a repo is now the point.
* **Re-running either with the same target** is a rotation, exactly as today: same id, same topic,
  same lanes, new secret (`registry.rs:435-447` already does this and is why history survives).
* It prints the handle and, for a named one, the single line a dispatcher must set:
  `KICKOFF_CHANNEL_CONVERSATION=ops`.
* **It writes nothing into any repo.** `write_token_file` (`registry.rs:581-607`) points at the
  channel space instead. Its `set_permissions(0o600)` re-assertion stays — the reason it exists (an
  existing file keeps its old mode through `.mode()`) is unchanged by the move.

**What is deleted, and this is the prize.** `secret_exposure`, `already_tracked`,
`inside_a_working_tree` and the refusal that writes nothing — 130 lines at `enroll.rs:37-61` and
`:193-305` — plus `--even-if-git-would-commit-it`. Not *passed*: **unreachable**. There is no file in
a tracked tree to check. Measured over the twelve adopted repos under `~/Projects`: ten would commit
`.kickoff/hub.token`, and one of those remotes is a published Obsidian plugin. Today going from three
projects to fourteen means editing ten `.gitignore` files first, or typing an override flag at a
keyboard ten times, in trees whose own coordinators commit and push unattended. After this, there is
nothing to refuse.

**One adjacent paragraph, take it or leave it.** If `enroll <folder>` resolves its target through
`--git-common-dir`'s parent before hashing, then `herdr-tg enroll <a lane worktree>` becomes a
*rotation of the main tree's conversation* rather than a second project. For the three live rows the
typed path already **is** the main top — verified by recomputation, all three ids match — so no id
moves. That kills `docs/TAXONOMY.md` §6 outright. It is separable from everything above.

---

## 6. Admission stays terminal-only — the argument, not the assertion

Constraint ① is the one with teeth, so here it is spelled out.

**A conversation can be created by exactly one thing: argv, at a keyboard.** Nothing else changes.
The socket cannot create one — `hello` gains no field, `admit` (`hub.rs:909-981`) gains no branch,
and an unknown token is refused with `unknown_project` exactly as today. Telegram cannot create one —
`resolve_tap` (`hub.rs:992`) still resolves against a written record and nothing on that path touches
the registry's write side. There is **no** proposal here for kickoff to ask the hub over the wire for
a new conversation, so the question "why is that not inbound content granting access" does not arise:
this design never asks it.

The env var is not a hole. It is read by the bridge, in the bridge's own process, and never reaches
the hub. A handle it names must already have a directory a terminal created, holding a secret a
terminal minted. Naming one that does not exist yields a bridge that refuses to dial.

**The honest cost.** The seed's conversation proposes a room; the operator taps yes on his phone;
**and then somebody has to reach a keyboard.** That is real friction and this design does not remove
it. Two answers, and only the first is shipped:

* **(a) He runs one command.** `herdr-tg enroll ~/Projects/acme --as ops`. Coherent with the
  taxonomy — a room is a thing he *sits in*, and deciding one exists is exactly the irreducible kind
  of decision. Coherent with `docs/CAPABILITIES.md` REQUIRES §1.
* **(b) A pool, already legal under the existing rule.** He pre-mints spares at a terminal —
  `herdr-tg enroll --as room-1 … room-8` — and kickoff, when a room is approved, **selects** one by
  handle. That is inbound content selecting from what the machine already knows, in the admission
  rule's own words, and the hub is not involved at all. Its cost is honest and appears in §9: a
  pooled room shows on the phone as `room-3` until a terminal changes its title.

---

## 7. The wire

**No frame changes. None.** `hello` still carries `{project_id, token, instance, repo, pid, lane?}`
(`crates/hub-proto/src/frame.rs:172-196`); `welcome` still echoes `{project, lane?, topic_id?,
limits}` (`:284-297`). `project_id` is still never consulted (`registry.rs:307`), `repo` is still
never read in `hub.rs`, `pid` is still discarded in favour of the socket's (`hub.rs:974`, `:1660`).
`hub-proto` is untouched; `hub.rs` is untouched.

**Does an old bridge still work?** Yes, throughout, and this is the part that makes the migration
boring. `resolve` (`registry.rs:309-319`) hashes the presented token and compares it against
`token_sha256`. It does not know or care which file the bridge read those bytes out of. So the
migration **does not rotate anything**: it *copies* the existing secret from
`<repo>/.kickoff/hub.token` into `conversations/<handle>/secret` and leaves the repo file in place.
Same bytes, two files, one hash, and the hub cannot tell an old bridge from a new one. No two-source
lookup, no fallback branch, no code in the hub at all.

Constraint ③, plainly: **this adds no seventh capability.** Identity is capability 1 and stays
capability 1. The hub allocates nothing, names nothing, and creates nothing it did not create
yesterday.

---

## 8. Migration — three live projects, four topics, no service touch

The asset that makes this safe: **topic bindings hang off the id, and the id does not move.**
`topic_id` and `lane_topics` are fields of `Project`, keyed by `ProjectId`. Verified by
recomputation against the live registry — `p-d08f8068ee56` = herdr-tg (topic 253),
`p-b0c13b72d117` = hub-dogfood (topic 255, lanes `hub-dogfood-lane-a` → 284 and
`hub-dogfood-lane-b` → 286), `p-62af70abc838` = oc-dogfood (topic 267). The unnamed mint rule in §5
is the same function on the same input, so those three ids are unchanged *by derivation* — and the
migration carries them verbatim anyway, so even a changed derivation could not orphan a topic.

Handles for the three come free: `handle = title`, and `unique_title` (`registry.rs:479`) already
guarantees no two rows share a title. Measured: `herdr-tg`, `hub-dogfood`, `oc-dogfood` — distinct.

Order, and the last two steps are the load-bearing ones:

1. **Assert 0700** on the state directory and on `conversations/`. Four lines. Nothing else works
   safely until this does. Reversible, no live effect.
2. **`handle` on the record**, `#[serde(default)]`. Nothing reads it yet.
3. **`herdr-tg adopt-secrets`**, one shot, no restart. For each enrolled row: create
   `conversations/<title>/`, copy the bytes of `<repo>/.kickoff/hub.token` into `secret` at 0600,
   write `default-for/<sha256(repo)[..16]>` → the handle, set `handle` on the row under the flock
   (`registry.rs:517`). `id`, `token_sha256`, `topic_id`, `lane_topics`, `icon_color`, `title`:
   untouched. **Nothing can break here**, because nothing the hub reads has changed — a crash halfway
   leaves some conversations with a second copy of a secret nobody is reading yet.
4. **The terminal door** writes the new place. `--as`, the pointer, the title rule. The git guard
   **stays** until step 7 — it is harmless once no secret is written into a repo, and removing it
   early removes the only protection for a box still on the old path.
5. **The bridges, last.** `where.ts` (`findConversation` replaces `findProject`; the hop at `:143`
   goes), `server.ts` (the env term, `enrolHint`, the refusal sentences), `bridge.ts:41`/`:186`.
   These take effect only when a session restarts — which is precisely why constraint ④ is satisfied
   by doing nothing: **the operator's live session keeps reading `<repo>/.kickoff/hub.token`, which
   step 3 left exactly where it was.**
6. **Delete the repo token files** — and only after a bridge has reconnected on the new path.
   Rollback before this point is `rm -rf conversations/ default-for/`; after it, it is
   `herdr-tg enroll <repo>`, and the topic comes back because the id never moved.
7. **Then** delete `TOKEN_FILE` (`registry.rs:37`), the git guard, the override flag, `.gitignore:51`,
   and the 48 token-writing lines across six test files. Retire the three orphan state files
   (`pushed.state.json`, `routing.state.json`, `keystrokes.audit.log`) while in there — they are the
   deleted scraper's, nothing reads them, and two of them hold the forum id and the operator's
   Telegram user id at rest.

`the_real_plugin_and_the_real_hub_agree_on_the_wire` (`hub/tests.rs:1598`) is the test that must
stay green across step 5: it writes a secret and starts the real bun bridge against the real hub. It
changes from writing `<tempdir>/herdr-tg/.kickoff/hub.token` to writing a tempdir channel space and
passing `KICKOFF_CHANNEL_CONVERSATION` — which is also what proves the bridge's new derivation is
overridable at all.

---

## 9. What this does NOT solve

Listed because the honest ones are the useful ones.

1. **It authenticates nothing new.** At one uid, any process can read any conversation's secret out
   of the channel space, exactly as it could read any repo's token. Measured: a 0600 file and a
   process's environment are equally readable to the same user, and `SO_PEERCRED`, `/proc/<pid>/cwd`,
   `environ`, `cgroup` and the `repo` field on the wire are all either chosen by the peer or by
   whoever spawned it. **Moving the secret changes who commits it to git, not who can steal it on the
   box.** Anyone wanting real separation wants the delegated-fd design, and this proposal does not
   compete with it — it is orthogonal and could be adopted underneath it.
2. **It is a new problem for containers with mapped uids.** A repo bind-mount was incidentally easy;
   a channel-space file has to be mounted deliberately. One file per container, read-only — better
   than a directory inside the working tree, but it *is* a new thing a dispatcher must do, and
   nothing here does it. `where.ts:96` / `bridge.ts:49` deriving the socket from their own `getuid()`
   still breaks first in a container, and this changes nothing about that.
3. **It does not start anything.** A room becomes *addressable*; seam ④ (who starts a container) is
   exactly as open as it was. `docs/CAPABILITIES.md` OPEN §1 is untouched.
4. **It does not remove the keyboard from the loop.** §6(a) is a real cost, and §6(b)'s pool has a
   real cost of its own: a pooled room appears on the phone as `room-3` until a terminal edits its
   title, because `title` is registry state and the registry is terminal-only. A handle *rename* is
   worse than it looks — it moves the secret's path, so a live bridge survives only until its next
   reconnect. Both are unsolved.
5. **Lane collision is untouched.** `bind_topic` is still a bare insert (`registry.rs:345`), nothing
   prunes `lane_topics`, and git only dedupes among *live* worktrees — so a recycled worktree name
   still inherits a dead conversation's topic (`docs/TAXONOMY.md` §2). Handles *are* deduped at the
   terminal door, so this design dedupes the new namespace and not the old one.
6. **The fan-in is still keyed on the repo.** `faninSocket(mainTop, lane)` (`where.ts:115`,
   `fanin.ts:74`) collides two conversations in one repo — which this design makes *reachable on
   purpose* rather than only by monorepo accident (`docs/TAXONOMY.md` §5). The fix is a one-line
   re-key onto `(handle, lane)`, this design supplies the handle that fix needs, and **it is not
   included here**. Anyone grafting this must take that line with it or the opencode path breaks the
   first day two rooms share a repo.
7. **The title clip still clips from the left** (`registry.rs:110`), calibrated for names that differ
   in their tails. Rooms are top-level rows so a room's own title is not clipped — but this design
   does not improve the lane case at all.
8. **Whether rooms should group under their org on the phone is left open.** Title-is-the-handle
   makes `ops` a sibling row of `acme`. At fourteen conversations he may want `acme · ops`. That is
   his call and it is a one-line change to the title rule either way.
9. **`asks.json`, the audit log and the heartbeat stay global.** The per-conversation directory is
   the seam for moving them; nothing moves now.
10. **It is not `~/.claude/channels/` literally.** It is one channel's own space, scoped by
    conversation inside it. Only one channel exists, that directory is empty, and a one-line env var
    is what would make the location literal if he wants it.

---

## 10. Cost

| file | change | shape |
| --- | --- | --- |
| `crates/kickoff-channel/src/lock.rs` | 0700 assertion, optional `HERDR_TG_STATE_DIR` | ~6 lines |
| `crates/kickoff-channel/src/registry.rs` (936 ln) | `handle`; `write_token_file` → channel space; pointer write; handle uniqueness + shape; containment guard scoped to unnamed | ~130 lines changed |
| `crates/kickoff-channel/src/cmd/enroll.rs` (663 ln) | `--as`; new printed lines; `adopt-secrets`; then −130 for the git guard | net **negative** |
| `crates/kickoff-channel/src/main.rs` | one flag, one subcommand | ~15 lines |
| `plugins/kickoff-channel/where.ts` (168 ln) | `findConversation` replaces `findProject`; `:143` deleted | net negative |
| `plugins/kickoff-channel/server.ts` (778 ln) | the env term, `enrolHint`, three refusal sentences | ~30 lines |
| `adapters/opencode-bridge/bridge.ts` (529 ln) | same, second time | ~20 lines |
| tests | 48 token-writing lines across 6 files; new RED tests for the handle door | ~150 lines |
| **`crates/kickoff-channel/src/hub.rs`** | **none** | 0 |
| **`crates/hub-proto/`** | **none** | 0 |

**The riskiest part is not the migration.** The migration cannot lose a topic (no id moves) and
cannot break a bridge (no hash changes). The riskiest part is that **a handle is now a path
segment**. A handle carrying `/` or `..` writes a secret outside the conversations directory, at a
terminal, with the operator's authority. It needs the same treatment `lane_is_addressable`
(`hub.rs:773`) already gives lane names — refused at the door, before a byte is minted, on shape
alone — *plus* a canonicalised containment assert before the write, because the shape check and the
write are in different functions and this repo has already shipped a defect of exactly that form.
RED before GREEN on both.

The second risk is quieter: a bridge that finds neither an env var nor a pointer now **fails closed
into silence** on a box where step 3 was skipped. That is the correct behaviour and it is also how a
channel goes dead without anyone noticing. Mitigated by the order — the pointer exists before any
bridge is upgraded — and by the refusal sentence naming the one terminal command that mends it.
