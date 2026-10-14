// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! Small async adapters and watch-stream helpers owned in-tree.
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

/// The [`watcher::Config`](kube::runtime::watcher::Config) every watch in this
/// crate uses: kube-runtime's defaults with the server-side timeout set to
/// [`WATCH_TIMEOUT_SECS`](crate::constants::WATCH_TIMEOUT_SECS).
///
/// The management and child clients carry a
/// [`K8S_API_TIMEOUT_SECS`](crate::constants::K8S_API_TIMEOUT_SECS) read
/// timeout, and kube-runtime's documented contract is that the watch timeout
/// must be lower than the client's read timeout. With the 290s default, a watch
/// that saw no event for 30 seconds was killed client-side, so a newly created
/// object could wait minutes to be reconciled (bug-008). With this config the
/// apiserver closes each watch first and kube-runtime resumes it from the last
/// `resourceVersion`, which costs one request per watch every
/// `WATCH_TIMEOUT_SECS` and loses nothing.
#[must_use]
pub fn watch_config() -> kube::runtime::watcher::Config {
    kube::runtime::watcher::Config::default().timeout(crate::constants::WATCH_TIMEOUT_SECS)
}

/// Whether a [`kube::runtime::watcher::Error`] is the expected churn of a
/// long-running watch rather than a fault worth an operator's attention.
///
/// A healthy watch ends without an error: the apiserver closes it when its
/// `timeoutSeconds` elapses ([`watch_config`]) and kube-runtime resumes it.
/// So almost nothing here is routine.
///
/// Routine:
/// - [`WatchError`](kube::runtime::watcher::Error::WatchError) with HTTP 410
///   `Gone`: the `resourceVersion` aged out of the watch cache, so the watcher
///   relists. Normal on a busy or long-idle cluster.
///
/// Not routine, and deliberately still `warn`:
/// - [`WatchFailed`](kube::runtime::watcher::Error::WatchFailed): the stream
///   broke rather than ended, through a dropped connection or a client-side
///   timeout. This was once classed as routine because it appeared several
///   times a minute on a healthy cluster; that was bug-008 (a 30s client read
///   timeout under a 290s watch), and demoting it would have hidden the defect.
/// - any other apiserver status, because 401 and 403 mean the controller's
///   credentials are wrong and 500 means the apiserver is unwell;
/// - `InitialListFailed` and `WatchStartFailed`, which happen before a stream
///   exists, so a persistent one means the watch never establishes;
/// - `NoResourceVersion`, a configuration fault that reconnecting cannot fix.
///
/// A routine classification lowers the log level only. It never suppresses the
/// message.
#[must_use]
pub fn is_routine_watch_restart(error: &kube::runtime::watcher::Error) -> bool {
    use kube::runtime::watcher::Error;

    /// HTTP 410, returned when the requested `resourceVersion` is no longer
    /// held in the apiserver's watch cache.
    const HTTP_GONE: u16 = 410;

    match error {
        Error::WatchError(status) => status.code == HTTP_GONE,
        Error::WatchFailed(_)
        | Error::InitialListFailed(_)
        | Error::WatchStartFailed(_)
        | Error::NoResourceVersion => false,
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
