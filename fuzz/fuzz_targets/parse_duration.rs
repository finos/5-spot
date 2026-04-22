// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // parse_duration takes untrusted input from spec.gracefulShutdownTimeout;
        // assert it never panics regardless of byte content.
        let _ = five_spot::reconcilers::parse_duration(s);
    }
});
