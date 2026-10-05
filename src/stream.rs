// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! Small async adapters owned in-tree rather than taken as dependencies.
//!
//! Per `.claude/rules/dependency-internalization.md`: where the surface we
//! actually use is small, stable and fully understood, owning it beats
//! tracking an upstream release cadence for it.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use tokio::sync::mpsc::Receiver;

/// A [`Stream`] over a [`tokio::sync::mpsc::Receiver`].
///
/// Replaces `tokio_stream::wrappers::ReceiverStream`, which was the **only**
/// thing this project used from `tokio-stream` (one symbol, from a 107-line
/// file, in a 3,037-line crate). Both controllers feed provider and target
/// watch events into `Controller::reconcile_on`, which wants a `Stream`, while
/// the watch managers produce an `mpsc::Receiver`.
///
/// ## Provenance
///
/// Behaviourally equivalent to `tokio-stream` 0.1.19's bounded-mpsc wrapper
/// (MIT, Tokio contributors), but written against `Receiver::poll_recv`
/// directly rather than copied, so no upstream code is vendored and no MIT
/// notice is carried. The contract it must honour, and which
/// `stream_tests.rs` pins:
///
/// - items arrive in send order;
/// - buffered items are drained **before** the end, not dropped with the
///   sender;
/// - the stream ends once every sender is gone, which is what stops
///   `reconcile_on` waiting on a dead channel forever;
/// - an open but empty channel is `Pending`, never end-of-stream;
/// - `None` is terminal and repeatable, never a panic.
pub struct ReceiverStream<T> {
    inner: Receiver<T>,
}

impl<T> ReceiverStream<T> {
    /// Wrap `inner` as a [`Stream`].
    #[must_use]
    pub const fn new(inner: Receiver<T>) -> Self {
        Self { inner }
    }
}

impl<T> Stream for ReceiverStream<T> {
    type Item = T;

    /// Delegates to [`Receiver::poll_recv`], which already provides every
    /// property above: ordering, draining after sender drop, `None` once
    /// closed and empty, and waker registration while open.
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.inner.poll_recv(cx)
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
