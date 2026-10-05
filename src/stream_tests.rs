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
}
