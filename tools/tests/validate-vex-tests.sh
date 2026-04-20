#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# End-to-end tests for tools/validate-vex.sh. Each fixture directory represents
# one schema or uniqueness case; we assert the expected exit code. Run directly
# or from CI:
#
#   ./tools/tests/validate-vex-tests.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
VALIDATE="$REPO_ROOT/tools/validate-vex.sh"
FIXTURES="$SCRIPT_DIR/fixtures"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAIL=0

run_case() {
    local name="$1"
    local dir="$2"
    local expected_exit="$3"
    local actual_exit=0
    "$VALIDATE" "$dir" >"$TMP/out" 2>&1 || actual_exit=$?

    if [ "$actual_exit" -eq "$expected_exit" ]; then
        echo "PASS: $name (exit=$actual_exit)"
        PASS=$((PASS + 1))
        return 0
    fi

    echo "FAIL: $name (expected exit=$expected_exit, got $actual_exit)" >&2
    sed 's/^/    /' "$TMP/out" >&2
    FAIL=$((FAIL + 1))
    return 0
}

# Missing-dir path: exercises `directory does not exist` branch.
run_case "missing-dir" "$TMP/does-not-exist" 1

# Happy paths.
run_case "empty-dir"      "$FIXTURES/empty-dir"      0
run_case "valid-single"   "$FIXTURES/valid-single"   0
run_case "valid-multiple" "$FIXTURES/valid-multiple" 0
run_case "valid-affected" "$FIXTURES/valid-affected" 0
# Non-CVE identifiers (GHSA + RUSTSEC) accepted since 2026-04-20 —
# first real encounter was GHSA-cq8v-f236-94qc (rand soundness) which
# ships without a CVE ID.
run_case "valid-ghsa"     "$FIXTURES/valid-ghsa"     0
run_case "valid-rustsec"  "$FIXTURES/valid-rustsec"  0

# Negative paths — one per validation rule.
run_case "malformed-toml"            "$FIXTURES/malformed-toml"            1
run_case "missing-cve"               "$FIXTURES/missing-cve"               1
run_case "missing-status"            "$FIXTURES/missing-status"            1
run_case "missing-products"          "$FIXTURES/missing-products"          1
run_case "missing-author"            "$FIXTURES/missing-author"            1
run_case "missing-timestamp"         "$FIXTURES/missing-timestamp"         1
run_case "invalid-cve-format"        "$FIXTURES/invalid-cve-format"        1
run_case "invalid-ghsa-format"       "$FIXTURES/invalid-ghsa-format"       1
run_case "invalid-rustsec-format"    "$FIXTURES/invalid-rustsec-format"    1
run_case "invalid-status"            "$FIXTURES/invalid-status"            1
run_case "empty-products"            "$FIXTURES/empty-products"            1
run_case "bad-timestamp"             "$FIXTURES/bad-timestamp"             1
run_case "missing-justification"     "$FIXTURES/missing-justification"     1
run_case "invalid-justification"     "$FIXTURES/invalid-justification"     1
run_case "missing-action-statement"  "$FIXTURES/missing-action-statement"  1
run_case "duplicate-cve"             "$FIXTURES/duplicate-cve"             1
run_case "duplicate-ghsa"            "$FIXTURES/duplicate-ghsa"            1

echo ""
echo "Results: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
