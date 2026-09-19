---
name: the-wire-already-carried-files
description: hub-proto has had a complete bidirectional file vocabulary since 6 September — a design session nearly bumped the wire for a capability that was already there, and the gap was one seam
metadata:
  type: project
---

When the operator asked for multimedia over the PWA (18 September), the obvious shape was a
hub-proto version bump every adapter and both engines would inherit. **It was already built.**
`crates/hub-proto/src/frame.rs` carries `MessageFile{kind,path,mime,bytes,filename,why}` on
`HubFrame::Message.files` going down, `SayFile{name,mime,filename,as}` on `Say` and `Done` going up,
an absolute outbox path on `Welcome`, `FileKind` with Photo/Document/Video/Animation/Audio/Voice, and
`AckWhy::NoFile`. Bytes never cross the wire — paths and names do, because a frame is 64 KiB and a
screenshot is a megabyte.

The gap was exactly one seam: `kickoff-door` and the answers drop. `docs/PWA-DOOR.md` said so as
policy — "the write door carries words only". So the feature costs no wire change, no plugin bump
and no adapter line, and a file attached in the PWA can reach an agent byte-identically to one sent
from Telegram.

**Why:** the file vocabulary was built for the Telegram surface and survived its retirement intact,
because `hub-proto` deliberately knows nothing about which surface carries it. The capability
outlived the product that motivated it, and nothing in the docs said so — `docs/PWA-DOOR.md` listed
`say` as carrying `text` alone while `say.file` had been on the ring since files existed.

**How to apply:** before proposing a wire change here, read `frame.rs` end to end. The contract docs
understate the wire in at least one place, so the code is the source of truth about what the wire can
already say. Related: [[clean-house-beats-compatibility]].
