# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# Schema + enum + uniqueness validator for .vex/<cve>.toml files.
# Invoked by tools/validate-vex.sh; kept as a separate file so the test driver
# can run it directly too. Uses only the Python standard library (tomllib is
# stdlib as of 3.11).

from __future__ import annotations

import datetime as _dt
import re
import sys
import tomllib
from pathlib import Path

VALID_STATUSES = frozenset(
    {"not_affected", "affected", "fixed", "under_investigation"}
)

# Per OpenVEX spec: https://github.com/openvex/spec
VALID_JUSTIFICATIONS = frozenset(
    {
        "component_not_present",
        "vulnerable_code_not_present",
        "vulnerable_code_not_in_execute_path",
        "vulnerable_code_cannot_be_controlled_by_adversary",
        "inline_mitigations_already_exist",
    }
)

# Fields required on every file regardless of status.
REQUIRED_ALWAYS = ("cve", "status", "products", "author", "timestamp")

# Accepted vulnerability identifiers. The TOML field is still named `cve`
# for backward compatibility with the existing .vex/ files, but the value
# may be any of:
#   - CVE-YYYY-NNNN+    (MITRE CVE)
#   - GHSA-xxxx-xxxx-xxxx  (GitHub Security Advisory, case-insensitive)
#   - RUSTSEC-YYYY-NNNN (RustSec advisory DB)
# First real non-CVE encounter was GHSA-cq8v-f236-94qc (rand soundness)
# which ships without a CVE ID, so CVE-only would make it unrepresentable.
_CVE_RE = re.compile(r"^CVE-\d{4}-\d{4,}$")
_GHSA_RE = re.compile(r"^GHSA(-[a-z0-9]{4}){3}$", re.IGNORECASE)
_RUSTSEC_RE = re.compile(r"^RUSTSEC-\d{4}-\d{4}$")


def _is_accepted_id(value: str) -> bool:
    return bool(
        _CVE_RE.match(value)
        or _GHSA_RE.match(value)
        or _RUSTSEC_RE.match(value)
    )

# RFC-3339 in UTC (Z suffix) — we intentionally keep this narrow so every VEX
# timestamp is directly comparable without timezone arithmetic.
_RFC3339_RE = re.compile(
    r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$"
)


def _is_rfc3339(value: object) -> bool:
    if isinstance(value, _dt.datetime):
        return True
    if not isinstance(value, str):
        return False
    return bool(_RFC3339_RE.match(value))


def validate_file(path: Path) -> tuple[str | None, list[str]]:
    """Validate one TOML file. Returns (canonical_cve_or_None, error_list)."""
    errors: list[str] = []
    try:
        with path.open("rb") as fh:
            doc = tomllib.load(fh)
    except OSError as exc:
        return None, [f"{path}: cannot read file: {exc}"]
    except tomllib.TOMLDecodeError as exc:
        return None, [f"{path}: malformed TOML: {exc}"]

    for field in REQUIRED_ALWAYS:
        if field not in doc:
            errors.append(f"{path}: missing required field '{field}'")

    # If the core set is incomplete, further checks cascade into noise.
    if errors:
        return None, errors

    cve = doc["cve"]
    status = doc["status"]
    products = doc["products"]
    timestamp = doc["timestamp"]
    author = doc["author"]

    if not isinstance(cve, str) or not _is_accepted_id(cve):
        errors.append(
            f"{path}: 'cve' must be a CVE-YYYY-NNNN+, "
            f"GHSA-xxxx-xxxx-xxxx, or RUSTSEC-YYYY-NNNN identifier, "
            f"got {cve!r}"
        )

    if status not in VALID_STATUSES:
        errors.append(
            f"{path}: 'status' must be one of "
            f"{sorted(VALID_STATUSES)}, got {status!r}"
        )

    if not isinstance(products, list) or not products:
        errors.append(f"{path}: 'products' must be a non-empty list")
    elif not all(isinstance(p, str) and p for p in products):
        errors.append(
            f"{path}: 'products' entries must be non-empty strings"
        )

    if not isinstance(author, str) or not author.strip():
        errors.append(f"{path}: 'author' must be a non-empty string")

    if not _is_rfc3339(timestamp):
        errors.append(
            f"{path}: 'timestamp' must be RFC-3339 UTC "
            f"(e.g. 2026-04-19T00:00:00Z), got {timestamp!r}"
        )

    if status == "not_affected":
        just = doc.get("justification")
        if just not in VALID_JUSTIFICATIONS:
            errors.append(
                f"{path}: status='not_affected' requires 'justification' "
                f"in {sorted(VALID_JUSTIFICATIONS)}, got {just!r}"
            )

    if status in ("affected", "under_investigation"):
        action = doc.get("action_statement")
        if not isinstance(action, str) or not action.strip():
            errors.append(
                f"{path}: status='{status}' requires a non-empty "
                f"'action_statement'"
            )

    canonical_cve = cve.upper() if isinstance(cve, str) else None
    return canonical_cve, errors


def validate_dir(target: Path) -> list[str]:
    """Validate every *.toml under `target`. Returns a list of error strings."""
    if not target.is_dir():
        return [f"{target}: directory does not exist"]

    errors: list[str] = []
    seen: dict[str, Path] = {}

    # Sorted to make error output deterministic.
    for path in sorted(target.glob("*.toml")):
        cve, file_errors = validate_file(path)
        errors.extend(file_errors)

        if cve is None:
            continue
        if cve in seen:
            errors.append(
                f"{path}: duplicate CVE {cve} "
                f"already declared in {seen[cve]}"
            )
        else:
            seen[cve] = path

    return errors


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(
            f"usage: {argv[0]} <directory>",
            file=sys.stderr,
        )
        return 2

    target = Path(argv[1])
    errors = validate_dir(target)

    if errors:
        for err in errors:
            print(err, file=sys.stderr)
        print(
            f"\nvalidate-vex: {len(errors)} error(s) in {target}",
            file=sys.stderr,
        )
        return 1

    print(f"validate-vex: OK ({target})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
