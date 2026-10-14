// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_yields_items_in_send_order() {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        for i in 0..4 {
            tx.send(i).await.expect("send");
        }
        drop(tx);
        let got: Vec<i32> = ReceiverStream::new(rx).collect().await;
        assert_eq!(got, vec![0, 1, 2, 3]);
    }

    /// The property the controller relies on: the stream ends when every
    /// sender is gone. A stream that never ends would keep
    /// `Controller::reconcile_on` waiting forever on a dead channel.
    #[tokio::test]
    async fn test_ends_when_all_senders_drop() {
        let (tx, rx) = tokio::sync::mpsc::channel::<i32>(1);
        drop(tx);
        let mut stream = ReceiverStream::new(rx);
        assert_eq!(stream.next().await, None);
    }

    /// Buffered items are delivered before the end, not discarded with the
    /// sender.
    #[tokio::test]
    async fn test_drains_buffered_items_after_the_sender_drops() {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tx.send("a").await.expect("send");
        tx.send("b").await.expect("send");
        drop(tx);
        let got: Vec<&str> = ReceiverStream::new(rx).collect().await;
        assert_eq!(got, vec!["a", "b"]);
    }

    /// Pending when empty but open: it must not report end-of-stream just
    /// because nothing has arrived yet.
    #[tokio::test]
    async fn test_pending_while_open_and_empty() {
        let (tx, rx) = tokio::sync::mpsc::channel::<i32>(1);
        let mut stream = ReceiverStream::new(rx);
        let poll = futures::poll!(stream.next());
        assert!(poll.is_pending(), "an open, empty channel must be Pending");
        tx.send(7).await.expect("send");
        assert_eq!(stream.next().await, Some(7));
    }

    /// Still `None` on every subsequent poll, never a panic.
    #[tokio::test]
    async fn test_stays_ended_after_the_first_none() {
        let (tx, rx) = tokio::sync::mpsc::channel::<i32>(1);
        drop(tx);
        let mut stream = ReceiverStream::new(rx);
        assert_eq!(stream.next().await, None);
        assert_eq!(stream.next().await, None);
    }

    // ========================================================================
    // is_routine_watch_restart - which watcher errors are expected churn
    // ========================================================================

    use kube::core::Status;
    use kube::runtime::watcher;

    /// Build the error kube-rs produces when the apiserver closes a long-poll,
    /// which is what every idle watch does once `timeoutSeconds` elapses.
    fn watch_failed() -> watcher::Error {
        watcher::Error::WatchFailed(kube::Error::Service(Box::new(std::io::Error::other(
            "error reading a body from connection",
        ))))
    }

    fn watch_error_with_code(code: u16) -> watcher::Error {
        watcher::Error::WatchError(Box::new(Status {
            code,
            message: "too old resource version".to_string(),
            ..Default::default()
        }))
    }

    #[test]
    fn test_stream_recycle_is_routine() {
        // The apiserver ends every watch when its timeout elapses, and any
        // transport hiccup ends it the same way. kube-runtime reconnects, so
        // this must not be logged as a fault.
        assert!(is_routine_watch_restart(&watch_failed()));
    }

    #[test]
    fn test_gone_410_is_routine() {
        // The resourceVersion aged out of the watch cache; the watcher relists.
        assert!(is_routine_watch_restart(&watch_error_with_code(410)));
    }

    #[test]
    fn test_other_apiserver_status_codes_are_not_routine() {
        // 401/403 mean the controller's credentials are wrong, which an
        // operator must see. 500 means the apiserver is unwell.
        for code in [401, 403, 404, 429, 500] {
            assert!(
                !is_routine_watch_restart(&watch_error_with_code(code)),
                "status {code} must not be treated as routine"
            );
        }
    }

    #[test]
    fn test_no_resource_version_is_not_routine() {
        // The resource does not support watch: a configuration fault that will
        // never resolve by reconnecting.
        assert!(!is_routine_watch_restart(
            &watcher::Error::NoResourceVersion
        ));
    }

    #[test]
    fn test_initial_list_and_watch_start_failures_are_not_routine() {
        // These happen before a stream exists. A persistent failure here means
        // the watch never establishes, so it stays at warn.
        let list = watcher::Error::InitialListFailed(kube::Error::Service(Box::new(
            std::io::Error::other("connection refused"),
        )));
        let start = watcher::Error::WatchStartFailed(kube::Error::Service(Box::new(
            std::io::Error::other("connection refused"),
        )));
        assert!(!is_routine_watch_restart(&list));
        assert!(!is_routine_watch_restart(&start));
    }
}
