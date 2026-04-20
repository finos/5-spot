# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# Assembles a single OpenVEX document from every .vex/<cve>.toml file.
#
# Output is written to stdout (or --output) as pretty-printed JSON with sorted
# keys, so the content is deterministic and diffable. Statement fields track
# the OpenVEX v0.2.0 spec (https://github.com/openvex/spec).
#
# Runs validate_vex first so the assembler never emits a document from a
# malformed source tree.

from __future__ import annotations

import argparse
import datetime as _dt
import json
import sys
import tomllib
from pathlib import Path

# Keep validate_vex reachable when this file is executed directly.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import validate_vex  # noqa: E402

OPENVEX_CONTEXT = "https://openvex.dev/ns/v0.2.0"
OPENVEX_VERSION = 1


def _ts_to_str(value: object) -> str:
    if isinstance(value, _dt.datetime):
        # Normalize to RFC-3339 UTC with a Z suffix regardless of source tz.
        if value.tzinfo is None:
            value = value.replace(tzinfo=_dt.timezone.utc)
        return (
            value.astimezone(_dt.timezone.utc)
            .strftime("%Y-%m-%dT%H:%M:%SZ")
        )
    return str(value)


def _load_statement(path: Path) -> dict:
    with path.open("rb") as fh:
        doc = tomllib.load(fh)

    # Preserve the identifier verbatim — CVE-YYYY-NNNN+ is uppercase by
    # MITRE convention, but GHSA-xxxx-xxxx-xxxx segments are lowercase,
    # and upper-casing them breaks round-tripping against osv.dev and
    # github.com/advisories (which treat GHSA IDs case-insensitively for
    # matching but render them in their canonical lowercase form).
    statement: dict = {
        "vulnerability": {"name": doc["cve"]},
        "products": [{"@id": product} for product in doc["products"]],
        "status": doc["status"],
        "timestamp": _ts_to_str(doc["timestamp"]),
    }

    for optional in ("justification", "impact_statement", "action_statement"):
        if optional in doc and doc[optional]:
            statement[optional] = doc[optional]

    return statement


def build_document(
    vex_dir: Path,
    doc_id: str,
    author: str,
    timestamp: str,
) -> dict:
    statements = [_load_statement(p) for p in sorted(vex_dir.glob("*.toml"))]
    return {
        "@context": OPENVEX_CONTEXT,
        "@id": doc_id,
        "author": author,
        "timestamp": timestamp,
        "version": OPENVEX_VERSION,
        "statements": statements,
    }


def _now_utc_rfc3339() -> str:
    return _dt.datetime.now(tz=_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vex-dir", required=True, type=Path)
    parser.add_argument(
        "--id",
        required=True,
        help="Canonical @id URI, e.g. "
        "https://github.com/<owner>/<repo>/releases/tag/<tag>/vex",
    )
    parser.add_argument(
        "--author",
        required=True,
        help="Document author (release actor / signer).",
    )
    parser.add_argument(
        "--timestamp",
        default=None,
        help="Override document timestamp (RFC-3339). Defaults to now().",
    )
    parser.add_argument(
        "--output",
        default=None,
        type=Path,
        help="Output file path. Defaults to stdout.",
    )
    args = parser.parse_args(argv[1:])

    errors = validate_vex.validate_dir(args.vex_dir)
    if errors:
        for err in errors:
            print(err, file=sys.stderr)
        print(
            f"\nassemble-openvex: refusing to emit — {len(errors)} "
            f"validation error(s) in {args.vex_dir}",
            file=sys.stderr,
        )
        return 1

    timestamp = args.timestamp or _now_utc_rfc3339()
    doc = build_document(args.vex_dir, args.id, args.author, timestamp)
    rendered = json.dumps(doc, indent=2, sort_keys=True) + "\n"

    if args.output:
        args.output.write_text(rendered)
    else:
        sys.stdout.write(rendered)

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
