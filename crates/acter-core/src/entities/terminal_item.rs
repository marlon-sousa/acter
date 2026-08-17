//! Entity/value: one element of the ordered stream a terminal engine emits — text it
//! resolved, or a shell-integration marker it recognized.
//!
//! Text, not bytes: carriage returns, colour changes and prompt repaints are already
//! resolved by the emulator, which is the stream DESIGN's auto-read threshold was always
//! about (it counts extracted text precisely because escape sequences would inflate a
//! byte count). These types will belong to the `TerminalEngine` port's contract once
//! that port exists in B3; they land as entities first because B1 set the precedent that
//! a trait is not declared ahead of its implementer (spec B2, decision 10).

use crate::Osc133Marker;

/// One item from the engine. A batch of these is what
/// [`BoundaryTracker::observe`](crate::BoundaryTracker::observe) takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalItem {
    /// Text the engine resolved, in stream order.
    Text(String),
    /// A shell-integration marker the engine recognized.
    Marker(Osc133Marker),
}
