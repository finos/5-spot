// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! Byte-level tests for the netlink proc connector parsers.
//!
//! Runnable on macOS (no socket required). Each test synthesises the
//! exact byte layout the Linux kernel writes for one event, feeds it
//! through the parser, and asserts the resulting [`ProcEvent`].

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;

    // ─────────────────────────────────────────────────────────────────
    // Helpers — build the wire layouts the kernel emits.
    //
    // cn_msg header (20 bytes):
    //   id.idx       u32 LE  - CN_IDX_PROC = 1
    //   id.val       u32 LE  - CN_VAL_PROC = 1
    //   seq          u32 LE
    //   ack          u32 LE
    //   len          u16 LE  - payload length
    //   flags        u16 LE
    //
    // proc_event payload (24 bytes for an exec):
    //   what         u32 LE  - PROC_EVENT_EXEC = 0x00000002
    //   cpu          u32 LE
    //   timestamp_ns u64 LE
    //   exec.pid     i32 LE
    //   exec.tgid    i32 LE
    // ─────────────────────────────────────────────────────────────────

    /// Build a minimal `cn_msg` wrapping a payload, with the standard
    /// proc-connector id `(idx=1, val=1)`.
    fn cn_msg_with_payload(payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20 + payload.len());
        buf.extend_from_slice(&CN_IDX_PROC.to_le_bytes()); // idx
        buf.extend_from_slice(&CN_VAL_PROC.to_le_bytes()); // val
        buf.extend_from_slice(&0u32.to_le_bytes()); // seq
        buf.extend_from_slice(&0u32.to_le_bytes()); // ack
        let len = u16::try_from(payload.len()).expect("payload fits");
        buf.extend_from_slice(&len.to_le_bytes()); // len
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags
        buf.extend_from_slice(payload);
        buf
    }

    /// Build a 24-byte `proc_event` for an exec event with the given pid/tgid.
    fn exec_event_bytes(pid: u32, tgid: u32) -> Vec<u8> {
        let mut buf = Vec::with_capacity(24);
        buf.extend_from_slice(&PROC_EVENT_EXEC_RAW.to_le_bytes()); // what
        buf.extend_from_slice(&0u32.to_le_bytes()); // cpu
        buf.extend_from_slice(&0u64.to_le_bytes()); // timestamp_ns
        buf.extend_from_slice(&pid.to_le_bytes()); // process_pid
        buf.extend_from_slice(&tgid.to_le_bytes()); // process_tgid
        buf
    }

    // ─────────────────────────────────────────────────────────────────
    // parse_proc_event
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn parse_proc_event_exec_returns_pid_tgid() {
        let bytes = exec_event_bytes(4242, 4242);
        let evt = parse_proc_event(&bytes).expect("valid exec event parses");
        assert_eq!(
            evt,
            ProcEvent::Exec {
                pid: 4242,
                tgid: 4242
            }
        );
    }

    #[test]
    fn parse_proc_event_exec_distinguishes_pid_from_tgid() {
        // Real-world: a thread within a multi-threaded process can have
        // pid != tgid. The detector should match against both views.
        let bytes = exec_event_bytes(4243, 4242);
        let evt = parse_proc_event(&bytes).expect("valid event");
        assert_eq!(
            evt,
            ProcEvent::Exec {
                pid: 4243,
                tgid: 4242
            }
        );
    }

    #[test]
    fn parse_proc_event_fork_is_classified_other() {
        // Build a FORK event (4 bytes what + 4 cpu + 8 timestamp + payload).
        // Payload size differs from exec but we only care about `what`.
        let mut bytes = Vec::with_capacity(24);
        bytes.extend_from_slice(&0x0000_0001u32.to_le_bytes()); // PROC_EVENT_FORK
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes()); // parent pid
        bytes.extend_from_slice(&0u32.to_le_bytes()); // parent tgid
        let evt = parse_proc_event(&bytes).expect("non-exec event still parses");
        assert!(
            matches!(evt, ProcEvent::Other { what: 0x0000_0001 }),
            "FORK event must classify as Other, got {evt:?}"
        );
    }

    #[test]
    fn parse_proc_event_exit_is_classified_other() {
        let mut bytes = Vec::with_capacity(24);
        bytes.extend_from_slice(&0x8000_0000u32.to_le_bytes()); // PROC_EVENT_EXIT
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let evt = parse_proc_event(&bytes).expect("exit event parses");
        assert!(matches!(evt, ProcEvent::Other { what: 0x8000_0000 }));
    }

    #[test]
    fn parse_proc_event_truncated_returns_error() {
        // Anything shorter than the proc_event header (16 bytes:
        // what + cpu + timestamp_ns) is unconditionally bad.
        let bytes = [0u8; 8];
        let err = parse_proc_event(&bytes).expect_err("truncated must error");
        assert!(
            matches!(err, NetlinkError::Truncated { .. }),
            "expected Truncated, got {err:?}"
        );
    }

    #[test]
    fn parse_proc_event_exec_truncated_payload_returns_error() {
        // Header is full (16 bytes) but the exec payload (8 bytes for
        // pid + tgid) is missing. Must error rather than read past end.
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&PROC_EVENT_EXEC_RAW.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        let err = parse_proc_event(&bytes).expect_err("missing exec payload must error");
        assert!(matches!(err, NetlinkError::Truncated { .. }));
    }

    #[test]
    fn parse_proc_event_empty_buffer_returns_error() {
        let err = parse_proc_event(&[]).expect_err("empty input must error");
        assert!(matches!(err, NetlinkError::Truncated { .. }));
    }

    // ─────────────────────────────────────────────────────────────────
    // parse_cn_msg
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn parse_cn_msg_extracts_payload_for_exec() {
        let payload = exec_event_bytes(7777, 7777);
        let frame = cn_msg_with_payload(&payload);
        let extracted = parse_cn_msg(&frame).expect("well-formed frame parses");
        assert_eq!(extracted, payload.as_slice());
    }

    #[test]
    fn parse_cn_msg_rejects_wrong_idx() {
        // A connector message for some other subsystem (e.g. CN_IDX_CIFS = 5)
        // must be rejected — we are not subscribed to it and a wrong-idx
        // frame is either a kernel bug or wire corruption.
        let mut frame = Vec::with_capacity(20);
        frame.extend_from_slice(&5u32.to_le_bytes()); // wrong idx
        frame.extend_from_slice(&CN_VAL_PROC.to_le_bytes());
        frame.extend_from_slice(&[0u8; 12]); // seq + ack + len + flags
        let err = parse_cn_msg(&frame).expect_err("wrong idx must error");
        assert!(
            matches!(err, NetlinkError::InvalidId { idx: 5, .. }),
            "expected InvalidId{{idx=5}}, got {err:?}"
        );
    }

    #[test]
    fn parse_cn_msg_rejects_wrong_val() {
        let mut frame = Vec::with_capacity(20);
        frame.extend_from_slice(&CN_IDX_PROC.to_le_bytes());
        frame.extend_from_slice(&99u32.to_le_bytes()); // wrong val
        frame.extend_from_slice(&[0u8; 12]);
        let err = parse_cn_msg(&frame).expect_err("wrong val must error");
        assert!(
            matches!(err, NetlinkError::InvalidId { val: 99, .. }),
            "expected InvalidId{{val=99}}, got {err:?}"
        );
    }

    #[test]
    fn parse_cn_msg_truncated_header_returns_error() {
        let frame = [0u8; 10]; // shorter than the 20-byte header
        let err = parse_cn_msg(&frame).expect_err("truncated header must error");
        assert!(matches!(err, NetlinkError::Truncated { .. }));
    }

    #[test]
    fn parse_cn_msg_payload_shorter_than_declared_returns_error() {
        // Header claims len=24 but the buffer only has 4 payload bytes —
        // must error rather than slice out of bounds.
        let mut frame = Vec::with_capacity(24);
        frame.extend_from_slice(&CN_IDX_PROC.to_le_bytes());
        frame.extend_from_slice(&CN_VAL_PROC.to_le_bytes());
        frame.extend_from_slice(&0u32.to_le_bytes()); // seq
        frame.extend_from_slice(&0u32.to_le_bytes()); // ack
        frame.extend_from_slice(&24u16.to_le_bytes()); // len = 24
        frame.extend_from_slice(&0u16.to_le_bytes()); // flags
        frame.extend_from_slice(&[0u8; 4]); // 4 bytes payload, not 24
        let err = parse_cn_msg(&frame).expect_err("short payload must error");
        assert!(matches!(err, NetlinkError::Truncated { .. }));
    }

    #[test]
    fn parse_cn_msg_zero_length_payload_is_valid() {
        // A keep-alive or boundary frame with no payload is benign —
        // not all messages carry an event.
        let frame = cn_msg_with_payload(&[]);
        let payload = parse_cn_msg(&frame).expect("zero-length payload parses");
        assert!(payload.is_empty());
    }

    // ─────────────────────────────────────────────────────────────────
    // End-to-end: cn_msg → proc_event for an EXEC event.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn cn_msg_then_proc_event_round_trip_for_exec() {
        let payload = exec_event_bytes(12345, 12345);
        let frame = cn_msg_with_payload(&payload);
        let extracted = parse_cn_msg(&frame).expect("frame parses");
        let evt = parse_proc_event(extracted).expect("event parses");
        assert_eq!(
            evt,
            ProcEvent::Exec {
                pid: 12345,
                tgid: 12345
            }
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Subscriber stub on non-Linux platforms.
    //
    // On macOS / Windows (where our CI also runs unit tests) the
    // Subscriber::new() entry point exists but immediately returns
    // NetlinkError::Unsupported. This pins that contract.
    // ─────────────────────────────────────────────────────────────────

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn subscriber_new_returns_unsupported_on_non_linux() {
        let err = Subscriber::new().expect_err("non-linux must refuse");
        assert!(
            matches!(err, NetlinkError::Unsupported),
            "expected Unsupported on non-linux, got {err:?}"
        );
    }
}
