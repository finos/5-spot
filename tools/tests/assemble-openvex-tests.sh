#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# Tests for tools/assemble_openvex.py. Covers the happy path (multiple valid
# statements), the validator-gate negative path, CLI argument errors, the
# --output flag, and normalization of TOML datetimes to RFC-3339 UTC strings.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ASSEMBLER="$REPO_ROOT/tools/assemble_openvex.py"
FIXTURES="$SCRIPT_DIR/fixtures"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAIL=0

fail_case() {
    echo "FAIL: $1" >&2
    sed 's/^/    /' "$2" >&2 || true
    FAIL=$((FAIL + 1))
}

pass_case() {
    echo "PASS: $1"
    PASS=$((PASS + 1))
}

# ── Happy path: assemble from valid-multiple ────────────────────────────────
out="$TMP/happy.json"
python3 "$ASSEMBLER" \
    --vex-dir "$FIXTURES/valid-multiple" \
    --id "https://github.com/owner/repo/releases/tag/v1.2.3/vex" \
    --author "erick.bourgeois@gmail.com" \
    --timestamp "2026-04-19T12:00:00Z" \
    --output "$out" >/dev/null 2>"$TMP/err"

if [ -s "$out" ] &&
   grep -q '"@context": "https://openvex.dev/ns/v0.2.0"' "$out" &&
   grep -q '"@id": "https://github.com/owner/repo/releases/tag/v1.2.3/vex"' "$out" &&
   grep -q '"author": "erick.bourgeois@gmail.com"' "$out" &&
   grep -q '"version": 1' "$out" &&
   grep -q '"CVE-2025-0001"' "$out" &&
   grep -q '"CVE-2025-0002"' "$out" &&
   grep -q '"pkg:oci/5-spot-chainguard"' "$out"; then
    pass_case "happy-path produces well-formed OpenVEX"
else
    fail_case "happy-path produces well-formed OpenVEX" "$out"
fi

# JSON must parse cleanly.
if python3 -c "import json,sys; json.loads(open('$out').read())" 2>/dev/null; then
    pass_case "happy-path output is valid JSON"
else
    fail_case "happy-path output is valid JSON" "$out"
fi

# ── Happy path via stdout (no --output) ─────────────────────────────────────
if python3 "$ASSEMBLER" \
    --vex-dir "$FIXTURES/valid-single" \
    --id "urn:vex:test" \
    --author "ci@example" \
    --timestamp "2026-04-19T00:00:00Z" > "$TMP/stdout.json" 2>"$TMP/err"; then
    if grep -q '"CVE-2025-0001"' "$TMP/stdout.json"; then
        pass_case "stdout output captures document"
    else
        fail_case "stdout output captures document" "$TMP/stdout.json"
    fi
else
    fail_case "stdout output exits 0" "$TMP/err"
fi

# ── Negative path: validator rejects malformed input ────────────────────────
set +e
python3 "$ASSEMBLER" \
    --vex-dir "$FIXTURES/invalid-status" \
    --id "urn:vex:test" \
    --author "ci@example" \
    --timestamp "2026-04-19T00:00:00Z" \
    --output "$TMP/bad.json" >/dev/null 2>"$TMP/err"
rc=$?
set -e
if [ "$rc" -eq 1 ] && [ ! -f "$TMP/bad.json" ]; then
    pass_case "invalid input rejected with exit=1 and no output written"
else
    fail_case "invalid input rejected with exit=1 and no output written" "$TMP/err"
fi

# ── Negative path: missing required CLI args ────────────────────────────────
set +e
python3 "$ASSEMBLER" --vex-dir "$FIXTURES/valid-single" >/dev/null 2>"$TMP/err"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
    pass_case "missing --id / --author rejected"
else
    fail_case "missing --id / --author rejected" "$TMP/err"
fi

# ── GHSA identifier case is preserved (not upper-cased) ────────────────────
# GHSA IDs are conventionally lowercase alphanumeric segments; upper-casing
# them changes the on-wire identifier and breaks a round-trip against
# osv.dev / github.com/advisories. Regression guard for the rand soundness
# advisory (GHSA-cq8v-f236-94qc) which was the first non-CVE ID we shipped.
out_ghsa="$TMP/ghsa.json"
python3 "$ASSEMBLER" \
    --vex-dir "$FIXTURES/valid-ghsa" \
    --id "urn:vex:ghsa-test" \
    --author "ci@example" \
    --timestamp "2026-04-19T00:00:00Z" \
    --output "$out_ghsa" >/dev/null 2>"$TMP/err"
if grep -q '"GHSA-cq8v-f236-94qc"' "$out_ghsa" &&
   ! grep -q '"GHSA-CQ8V-F236-94QC"' "$out_ghsa"; then
    pass_case "GHSA identifier case is preserved verbatim"
else
    fail_case "GHSA identifier case is preserved verbatim" "$out_ghsa"
fi

# ── Default timestamp (no --timestamp) produces RFC-3339 UTC ───────────────
out_dt="$TMP/dt.json"
python3 "$ASSEMBLER" \
    --vex-dir "$FIXTURES/valid-single" \
    --id "urn:vex:test" \
    --author "ci@example" \
    --output "$out_dt" >/dev/null 2>"$TMP/err"
if grep -Eq '"timestamp": "[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z"' "$out_dt"; then
    pass_case "default timestamp is RFC-3339 UTC"
else
    fail_case "default timestamp is RFC-3339 UTC" "$out_dt"
fi

echo ""
echo "Results: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
