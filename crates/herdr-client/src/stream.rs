//! [`EventStream`] — an open `events.subscribe` connection, decoded frame by frame.
//!
//! # The no-self-heal contract, and why it is a feature
//!
//! `None` means **the server closed the stream**, and this client NEVER reconnects itself.
//! Two independent reasons, both load-bearing:
//!
//! 1. PLAN.md's failure table demands *"a single recovery notice when the stream re-establishes
//!    (not one per retry)"*. That is unimplementable if the client hides the drop — the binary
//!    cannot report a recovery it never saw fail.
//! 2. Subscribing replays a rolling roster backlog, so a silent internal reconnect would
//!    re-deliver history as if it were new edges. A phantom-push burst, once per hiccup.
//!
//! Disconnect is therefore a first-class observable the binary owns. [`EventStream::subscriptions`]
//! hands back exactly what the stream was opened with, so the reconnect loop can re-issue the set
//! verbatim (plus any pane that appeared meanwhile — `events.subscribe` freezes its set at connect
//! and there is no `events.update`).
//!
//! # A decode failure does NOT end the stream
//!
//! One unparseable frame yields `Some(Err(..))` and the stream keeps going. Only EOF and a real I/O
//! error are terminal. A bridge that has to run unattended for weeks must not be killed by one bad
//! line, and the alternative — swallowing it — is the failure this crate exists to prevent.
//!
//! # The byte ceiling is per FRAME, not per stream
//!
//! `transport::open_stream` builds the reader as `Lines` over `BufReader::new(take(N))`, which is
//! what stops one pathological unterminated line from OOM-ing the bridge. But `Take` counts for the
//! LIFETIME of the reader, and this reader lives as long as the connection — so left alone, `N`
//! would be a ceiling on the *whole stream*: after `N` cumulative bytes the reader reports EOF and
//! `poll_next` yields `None`, which this module has just finished declaring means *the server
//! closed*. The binary would report a disconnect that never happened, issue its recovery notice,
//! reconnect, and replay the roster backlog — the phantom burst, from a bridge that was healthy.
//!
//! Measured, so the size of the hole is on the record rather than assumed: the captured roster
//! frames in `tests/fixtures/events-mixed.ndjson` average **613 bytes**, so a 32 MiB lifetime
//! ceiling is ~55 000 frames — days at a human pace, but only ~90 minutes if anything ever adds the
//! `pane.focused` firehose (~10 frames/s) to a subscription set. And the failure is not reliably
//! loud: `tokio::io::Lines::poll_next_line` returns `Ok(None)` when the read returns 0 **and** its
//! buffer is empty, so a ceiling that happens to land on a `\n` boundary ends the stream in
//! complete silence.
//!
//! So [`EventStream::poll_next`] restores the limit after every complete line. The ceiling then
//! means what `crate::MAX_RESPONSE_BYTES` says it means — a bound on a single frame — the stream
//! lives as long as the connection does, and `None` keeps its one meaning. The OOM protection is
//! unchanged: a single line can still never exceed the ceiling, which
//! `tests::a_single_unterminated_line_is_still_bounded` pins.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;

use crate::error::HerdrError;
use crate::proto::event::{EVENTS_SUBSCRIBE, Event, Subscription, decode_event};
use crate::transport::EventLines;

/// An open event stream: the connection, its reader, and the subscription set it was opened with.
///
/// Dropping it closes the connection. There is no `close()` — a bridge that wants to stop listening
/// drops the value, and one that wants to re-subscribe opens a new stream.
#[derive(Debug)]
pub struct EventStream {
    lines: EventLines,
    subscriptions: Vec<Subscription>,
    /// The byte ceiling `transport::open_stream` opened the reader with, restored after every
    /// complete line — see [`EventStream::poll_next`]. Read off the reader itself rather than
    /// passed in, so it cannot drift from the limit `transport` actually set.
    ceiling: u64,
    /// Latched at EOF / I/O error so a stream polled past its end keeps answering `None` rather
    /// than poking a dead socket.
    finished: bool,
}

impl EventStream {
    /// Built by `HerdrClient::subscribe` **after** it has consumed the `subscription_started` ack,
    /// so the ack can never leak out of `poll_next` as an event.
    pub(crate) fn new(mut lines: EventLines, subscriptions: Vec<Subscription>) -> Self {
        let ceiling = lines.get_ref().get_ref().limit();
        EventStream {
            lines,
            subscriptions,
            ceiling,
            finished: false,
        }
    }

    /// The set this stream was opened with, verbatim.
    ///
    /// `events.subscribe` FREEZES the set at connect and there is no `events.update`, so a newly
    /// created pane requires tearing this stream down and opening a new one — the binary re-issues
    /// these plus the new pane's entry.
    pub fn subscriptions(&self) -> &[Subscription] {
        &self.subscriptions
    }

    /// The next frame, or `None` once the server has closed the stream.
    ///
    /// A thin wrapper over [`Stream::poll_next`], provided so a caller does not have to hand-roll a
    /// `poll_fn`: this crate deliberately depends on `futures-core` only, with no `StreamExt` and
    /// no `tokio-stream` in the graph.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. It holds no state of its own, and the partial-line state lives inside
    /// `tokio::io::Lines`, whose `next_line` is documented cancel-safe in tokio 1.53.1. Dropping
    /// this future in a `tokio::select!` loses nothing — which is the entire reason the reader is
    /// `Lines` and not `AsyncBufReadExt::read_line` ("data may have been partially read, and this
    /// data is lost").
    pub async fn next(&mut self) -> Option<Result<Event, HerdrError>> {
        std::future::poll_fn(|cx| Pin::new(&mut *self).poll_next(cx)).await
    }
}

impl Stream for EventStream {
    type Item = Result<Event, HerdrError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.finished {
            return Poll::Ready(None);
        }

        match Pin::new(&mut this.lines).poll_next_line(cx) {
            Poll::Pending => Poll::Pending,
            // EOF: the server closed. Terminal, and deliberately NOT retried — see the module docs.
            Poll::Ready(Ok(None)) => {
                this.finished = true;
                tracing::debug!("herdr closed the event stream");
                Poll::Ready(None)
            }
            Poll::Ready(Ok(Some(line))) => {
                // Restore the budget now that a whole line is in hand. `Take` counts for the
                // LIFETIME of the reader, so left alone `MAX_RESPONSE_BYTES` would be a ceiling on
                // the entire stream rather than on one frame — and a bridge that is supposed to run
                // unattended for weeks would hit it and then report a disconnect that never
                // happened. See the module docs.
                this.lines.get_mut().get_mut().set_limit(this.ceiling);
                Poll::Ready(Some(decode_event(&line)))
            }
            // A real I/O fault (ECONNRESET is the common one) is terminal too, but unlike EOF it is
            // reported: "the server closed cleanly" and "the socket broke" are different things to
            // put in front of an operator holding only a phone.
            Poll::Ready(Err(source)) => {
                this.finished = true;
                Poll::Ready(Some(Err(HerdrError::Io {
                    method: EVENTS_SUBSCRIBE,
                    source,
                })))
            }
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// Crate-private tests: these need to build an `EventStream` over a reader with a deliberately tiny
// ceiling, which `EventStream::new` and `EventLines` are both `pub(crate)` for. The spec's
// `tests/events.rs` table drives the public API and lives in `tests/events.rs`.
//
// `UnixStream::pair()` — no socket file, no TempDir, nothing that could reach the operator's herd.
// ───────────────────────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    use super::*;

    /// One well-formed frame, 113 bytes on the wire.
    const FRAME: &str = r#"{"event":"pane.agent_status_changed","data":{"pane_id":"wA:p1","workspace_id":"wA","agent_status":"blocked"}}"#;

    /// A ceiling far below the total the test pushes through it, so a LIFETIME cap cannot survive
    /// this test and a per-frame cap cannot fail it.
    const TINY: u64 = 200;

    fn stream_over(client: UnixStream, ceiling: u64) -> EventStream {
        EventStream::new(BufReader::new(client.take(ceiling)).lines(), Vec::new())
    }

    /// The ceiling bounds ONE FRAME, not the stream's lifetime.
    ///
    /// Eight frames — 912 bytes — through a 200-byte ceiling. With `Take`'s own cumulative
    /// semantics the reader reports EOF partway through the second frame and the stream ends early,
    /// which the binary cannot tell apart from herdr closing the connection. See the module docs.
    #[tokio::test]
    async fn the_ceiling_bounds_one_frame_not_the_streams_lifetime() {
        let (client, mut server) = UnixStream::pair().expect("a socketpair");
        let writer = tokio::spawn(async move {
            for _ in 0..8 {
                server.write_all(FRAME.as_bytes()).await.expect("write");
                server.write_all(b"\n").await.expect("write");
            }
            // Dropping the server half here is what produces the REAL EOF this test expects.
        });

        let mut stream = stream_over(client, TINY);
        let mut n = 0usize;
        while let Some(item) = stream.next().await {
            match item {
                Ok(Event::AgentStatus(_)) => n += 1,
                other => panic!("frame {n} came back as {other:?}"),
            }
        }
        writer.await.expect("the writer task");

        assert!(
            (FRAME.len() as u64 + 1) * 8 > TINY,
            "the test must actually push more than the ceiling through the stream"
        );
        assert_eq!(
            n, 8,
            "every frame must arrive: a ceiling on the stream's LIFETIME would end it early and \
             the binary would report a disconnect herdr never performed"
        );
    }

    /// ...and the OOM protection the ceiling exists for is still in force.
    ///
    /// 4 KiB with no newline anywhere — the shape that would grow an unbounded reader's buffer
    /// without limit. Every chunk handed back must be within the ceiling, and each one is a loud
    /// decode error carrying its bytes rather than silence.
    #[tokio::test]
    async fn a_single_unterminated_line_is_still_bounded() {
        let (client, mut server) = UnixStream::pair().expect("a socketpair");
        let writer = tokio::spawn(async move {
            server.write_all(&vec![b'x'; 4096]).await.expect("write");
        });

        let mut stream = stream_over(client, TINY);
        let mut chunks = 0usize;
        while let Some(item) = stream.next().await {
            let err = item.expect_err("a wall of `x` is not JSON");
            match &err {
                HerdrError::Decode { line, .. } => assert!(
                    line.len() as u64 <= TINY,
                    "chunk {chunks} was {} bytes, over the {TINY}-byte ceiling: memory is no \
                     longer bounded",
                    line.len()
                ),
                other => panic!("expected a Decode error, got {other:?}"),
            }
            chunks += 1;
        }
        writer.await.expect("the writer task");

        assert!(
            chunks >= 4096 / TINY as usize,
            "the unterminated line must be delivered in bounded pieces, not swallowed: {chunks}"
        );
    }

    /// Cancelling a poll loses nothing — the property slice 3's `tokio::select!` rests on.
    ///
    /// Spec delta #30: `Lines::next_line` is cancel-safe because the partial-line state lives in the
    /// `Lines` struct rather than in the future, while `AsyncBufReadExt::read_line` explicitly is
    /// not ("data may have been partially read, and this data is lost"). Slice 3 will select this
    /// stream against teloxide's long-poll, so a lost partial line is a lost ask — which makes this
    /// a correctness property and not a style note, and therefore something to prove rather than
    /// document.
    ///
    /// Half a frame is written, the poll is cancelled by a timeout, then the rest arrives. The
    /// `limit()` assertion is what makes the test meaningful: it proves the cancelled poll really
    /// had pulled those bytes off the socket, so they are exactly the bytes a non-cancel-safe reader
    /// would have dropped on the floor.
    #[tokio::test]
    async fn cancelling_a_poll_does_not_lose_a_partially_read_frame() {
        let (client, mut server) = UnixStream::pair().expect("a socketpair");
        let (head, tail) = FRAME.split_at(56);

        let mut stream = stream_over(client, TINY);
        server.write_all(head.as_bytes()).await.expect("write");

        // The cancellation: `timeout` drops the `next()` future while the frame is incomplete.
        let cancelled = tokio::time::timeout(Duration::from_millis(50), stream.next()).await;
        assert!(
            cancelled.is_err(),
            "half a frame must not decode as anything: {cancelled:?}"
        );
        let held = stream.lines.get_ref().get_ref().limit();
        assert_eq!(
            TINY - head.len() as u64,
            held,
            "the cancelled poll must already have consumed the head off the socket — otherwise \
             this test proves nothing about what a cancellation costs"
        );

        server.write_all(tail.as_bytes()).await.expect("write");
        server.write_all(b"\n").await.expect("write");
        drop(server);

        match stream.next().await {
            Some(Ok(Event::AgentStatus(a))) => {
                assert_eq!(a.pane_id, crate::PaneId::new("wA:p1"));
                assert_eq!(a.agent_status, crate::AgentStatus::Blocked);
            }
            other => panic!("the cancelled read lost the head of the frame: {other:?}"),
        }
        assert!(stream.next().await.is_none(), "then EOF");
    }

    /// EOF latches: a stream polled past its end keeps answering `None` instead of poking a socket
    /// the server has already closed.
    #[tokio::test]
    async fn eof_latches_and_the_reader_is_not_polled_again() {
        let (client, server) = UnixStream::pair().expect("a socketpair");
        drop(server);

        let mut stream = stream_over(client, TINY);
        assert!(stream.next().await.is_none());
        assert!(stream.next().await.is_none());
        assert!(stream.subscriptions().is_empty());
    }
}
