// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let single = vec![s.to_string()];
        let _ = five_spot::crd::parse_hour_ranges(&single);

        if let Some(mid) = s.find(';') {
            let multi = vec![s[..mid].to_string(), s[mid + 1..].to_string()];
            let _ = five_spot::crd::parse_hour_ranges(&multi);
        }
    }
});
