//! NDJSON framing: one JSON object per line, LF-terminated, UTF-8, bounded.
//!
//! Three rules carried over from this repo's herdr client, because each one cost a real debugging
//! session and none of them is obvious from reading a spec.
//!
//! **1. The trailing newline is this module's job, never a caller's.** [`write_frame`] is the only
//! place in the crate that appends `\n`, and there is no public path that can send an unterminated
//! line. Omitting it against herdr made the far side hang forever with no error and no close —
//! measured at 5.01 s, zero bytes read, connection still open. Only a client-side timeout catches
//! that, which for a phone-only operator is the worst failure the product has.
//!
//! **2. Read exactly one line. Never to EOF.** Reading to EOF surfaces a connection reset *after* a
//! perfectly good frame has already arrived, and the frame is lost with it.
//!
//! **3. Restore the byte ceiling after every complete line.** `Take` counts for the LIFETIME of the
//! reader, so a ceiling set once is a ceiling on the whole conversation rather than on one frame.
//! Left alone it ends a healthy stream in silence that reads as a disconnect the peer never
//! performed. [`FrameReader`] restores it after each frame; do not remove that.
//!
//! # Oversize is refused, never truncated
//!
//! A frame over [`MAX_FRAME_BYTES`] is a protocol error: the hub answers `refused` and closes.
//! Truncating instead would hand the operator half a sentence and no way to know it was half.
//! The three ways a read can end — clean close, a line that never terminated, and a line past the
//! ceiling — are three different errors here, because treating any of them as another is how a
//! disconnect gets reported that never happened.

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, Take,
};

use crate::MAX_FRAME_BYTES;
use crate::error::ProtoError;
use crate::frame::Envelope;

/// Reads one frame per call from a byte stream, bounded, and never past the end of a line.
pub struct FrameReader<R> {
    inner: BufReader<Take<R>>,
    /// The per-frame ceiling, restored after every complete line. See rule 3 in the module docs.
    ceiling: u64,
    buf: Vec<u8>,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    /// Wraps a stream at the default ceiling.
    pub fn new(reader: R) -> Self {
        Self::with_ceiling(reader, MAX_FRAME_BYTES as u64)
    }

    /// Wraps a stream at a chosen ceiling. Tests use a small one so the oversize path is cheap to
    /// exercise; nothing else should need it.
    pub fn with_ceiling(reader: R, ceiling: u64) -> Self {
        Self {
            inner: BufReader::new(reader.take(ceiling)),
            ceiling,
            buf: Vec::with_capacity(1024),
        }
    }

    /// The next frame, or `None` when the peer closed cleanly between frames.
    ///
    /// A close *between* frames is ordinary and is not an error. A close *inside* one is
    /// [`ProtoError::Unterminated`], because the bytes that did arrive are not a message and
    /// pretending otherwise is how half a sentence reaches a phone.
    pub async fn next<P: DeserializeOwned>(&mut self) -> Result<Option<Envelope<P>>, ProtoError> {
        self.buf.clear();
        let n = self
            .inner
            .read_until(b'\n', &mut self.buf)
            .await
            .map_err(ProtoError::Io)?;

        // Rule 3. Before any early return, so every exit path leaves the next frame a full budget.
        self.inner.get_mut().set_limit(self.ceiling);

        if n == 0 {
            return Ok(None);
        }
        if !self.buf.ends_with(b"\n") {
            // No terminator. Either the peer vanished mid-line, or the line ran past the ceiling
            // and `Take` handed us a synthetic EOF. These are different failures and the operator
            // is told different things about them, so they are not merged.
            return if self.buf.len() as u64 >= self.ceiling {
                Err(ProtoError::Oversize {
                    max: self.ceiling as usize,
                })
            } else {
                Err(ProtoError::Unterminated {
                    bytes: self.buf.len(),
                })
            };
        }
        // A terminated line that is itself over the ceiling. Reachable when the ceiling and the
        // frame length coincide exactly, and cheap to check, so it is checked rather than reasoned
        // about.
        if self.buf.len() > self.ceiling as usize {
            return Err(ProtoError::Oversize {
                max: self.ceiling as usize,
            });
        }

        let line = &self.buf[..self.buf.len() - 1];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        serde_json::from_slice(line)
            .map(Some)
            .map_err(|source| ProtoError::Decode {
                source,
                len: line.len(),
            })
    }
}

/// Writes one frame, newline included.
///
/// **THE ONLY WRITER.** Rule 1 in the module docs: the `\n` is appended here and nowhere else in
/// this crate, so no caller can forget it and no public path can send an unterminated line.
///
/// An oversize frame is rejected **before** anything is written. A partial write would leave the
/// connection carrying half an object, which the far side would either hang on or reject as
/// garbage — and neither tells anyone what actually happened.
pub async fn write_frame<W, P>(writer: &mut W, frame: &Envelope<P>) -> Result<(), ProtoError>
where
    W: AsyncWrite + Unpin,
    P: Serialize,
{
    let body = serde_json::to_vec(frame).map_err(ProtoError::Encode)?;

    // Compact `serde_json` escapes newlines inside strings, so a raw one can only come from a
    // hand-built body — a bug in this crate, not something a peer can cause.
    debug_assert!(
        !body.contains(&b'\n'),
        "frame body contains a raw newline; it would frame as two frames"
    );

    if body.len() + 1 > MAX_FRAME_BYTES {
        return Err(ProtoError::Oversize {
            max: MAX_FRAME_BYTES,
        });
    }

    let mut framed = Vec::with_capacity(body.len() + 1);
    framed.extend_from_slice(&body);
    framed.push(b'\n');

    writer.write_all(&framed).await.map_err(ProtoError::Io)?;
    writer.flush().await.map_err(ProtoError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{BridgeFrame, HubFrame};
    use crate::ids::FrameId;

    fn env(text: &str) -> Envelope<BridgeFrame> {
        Envelope::new(
            FrameId::new("f1"),
            BridgeFrame::Say {
                text: text.into(),
                hint: None,
                file: None,
            },
        )
    }

    #[tokio::test]
    async fn a_written_frame_always_ends_in_exactly_one_newline() {
        let mut out = Vec::new();
        write_frame(&mut out, &env("hello")).await.expect("writes");
        assert_eq!(out.iter().filter(|b| **b == b'\n').count(), 1);
        assert_eq!(*out.last().expect("non-empty"), b'\n');
    }

    #[tokio::test]
    async fn a_newline_inside_the_text_does_not_become_a_second_frame() {
        let mut out = Vec::new();
        write_frame(&mut out, &env("one\ntwo"))
            .await
            .expect("writes");
        assert_eq!(
            out.iter().filter(|b| **b == b'\n').count(),
            1,
            "an agent's multi-line message must stay one frame"
        );
        let mut r = FrameReader::new(&out[..]);
        let back: Envelope<BridgeFrame> = r.next().await.expect("reads").expect("a frame");
        assert_eq!(
            back.payload,
            BridgeFrame::Say {
                text: "one\ntwo".into(),
                hint: None,
                file: None,
            }
        );
    }

    #[tokio::test]
    async fn frames_round_trip_back_to_back_on_one_stream() {
        let mut out = Vec::new();
        for t in ["a", "b", "c"] {
            write_frame(&mut out, &env(t)).await.expect("writes");
        }
        let mut r = FrameReader::new(&out[..]);
        for t in ["a", "b", "c"] {
            let f: Envelope<BridgeFrame> = r.next().await.expect("reads").expect("a frame");
            assert_eq!(
                f.payload,
                BridgeFrame::Say {
                    text: t.into(),
                    hint: None,
                    file: None,
                }
            );
        }
        assert!(
            r.next::<BridgeFrame>().await.expect("reads").is_none(),
            "a clean close between frames is not an error"
        );
    }

    #[tokio::test]
    async fn the_ceiling_bounds_one_frame_and_not_the_whole_conversation() {
        // Rule 3, pinned. Without the restore, the total of all frames would be measured against
        // the ceiling and this stream would end early — reported as a disconnect the peer never
        // performed. Eight frames through a ceiling only three of them would fit under.
        let mut out = Vec::new();
        for _ in 0..8 {
            write_frame(&mut out, &env("0123456789"))
                .await
                .expect("writes");
        }
        let one = out.len() / 8;
        let mut r = FrameReader::with_ceiling(&out[..], (one * 3) as u64);
        let mut seen = 0;
        while let Some(_f) = r.next::<BridgeFrame>().await.expect("no early end") {
            seen += 1;
        }
        assert_eq!(
            seen, 8,
            "the stream ended early: the ceiling was cumulative"
        );
    }

    #[tokio::test]
    async fn a_frame_over_the_ceiling_is_refused_and_never_truncated() {
        let mut out = Vec::new();
        write_frame(&mut out, &env(&"x".repeat(500)))
            .await
            .expect("writes");
        let mut r = FrameReader::with_ceiling(&out[..], 64);
        match r.next::<BridgeFrame>().await {
            Err(ProtoError::Oversize { max }) => assert_eq!(max, 64),
            other => panic!("expected an oversize refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_peer_that_vanishes_mid_frame_is_not_reported_as_a_clean_close() {
        let mut out = Vec::new();
        write_frame(&mut out, &env("hello")).await.expect("writes");
        out.truncate(out.len() - 3); // no terminator, well under the ceiling
        let mut r = FrameReader::new(&out[..]);
        match r.next::<BridgeFrame>().await {
            Err(ProtoError::Unterminated { bytes }) => assert!(bytes > 0),
            other => panic!("expected an unterminated frame, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn writing_a_frame_larger_than_the_ceiling_fails_before_any_byte_goes_out() {
        let mut out = Vec::new();
        let huge = env(&"x".repeat(MAX_FRAME_BYTES + 10));
        assert!(matches!(
            write_frame(&mut out, &huge).await,
            Err(ProtoError::Oversize { .. })
        ));
        assert!(
            out.is_empty(),
            "a refused frame must not leave half an object on the wire"
        );
    }

    #[tokio::test]
    async fn a_line_that_is_not_json_names_itself_a_decode_failure() {
        let mut r = FrameReader::new(&b"{not json}\n"[..]);
        assert!(matches!(
            r.next::<BridgeFrame>().await,
            Err(ProtoError::Decode { .. })
        ));
    }

    #[tokio::test]
    async fn a_hub_frame_reads_back_as_the_same_hub_frame() {
        let mut out = Vec::new();
        let f = Envelope::new(FrameId::new("f2"), HubFrame::Ping);
        write_frame(&mut out, &f).await.expect("writes");
        let mut r = FrameReader::new(&out[..]);
        let back: Envelope<HubFrame> = r.next().await.expect("reads").expect("a frame");
        assert_eq!(back, f);
    }
}
