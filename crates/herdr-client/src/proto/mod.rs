//! Wire types: the request bodies, the reply envelope, the domain model, and the event decoder.
//!
//! # PARTIAL — build order step 7
//!
//! Landed: [`model`] (the domain types herdr's `session.snapshot` / `pane.read` return),
//! `response` (the reply envelope plus the per-method result wrappers), `request` (the envelope
//! plus the five read-side params types and `events.subscribe`) and [`event`] (the two-step event
//! decoder and the `Subscription` type). Still to land: the three write methods' params types
//! (step 8).
//!
//! Hand-written on purpose. `herdr api schema --json` is kept as a drift-test **fixture**, never as
//! a codegen source: it under-declares (91 methods against 92 on the wire — `pane.graphics.stream`
//! is missing) and over-declares (`EventMatch` lists 19 variants while `events.wait` rejects all but
//! one). `tests/schema_drift.rs` is where the schema gets to disagree with us, loudly, at
//! `cargo test`, instead of at 2 a.m. through a missed ask.

pub mod event;
pub mod model;
pub(crate) mod request;
pub(crate) mod response;
