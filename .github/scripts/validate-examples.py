#!/usr/bin/env python3
# Copyright (c) 2026 Erick Bourgeois, 5-Spot
# SPDX-License-Identifier: Apache-2.0
"""Validate every example and documentation manifest against a real API server.

Why this exists (ADR 0019): `bootstrapSpec` and `infrastructureSpec` are
deliberately unstructured in the ScheduledMachine CRD, so validating a
ScheduledMachine says nothing about whether the provider object inside it is
valid. A field the provider renamed between API versions passes, then is
silently dropped when 5-Spot creates the object. The only check that catches it
is the provider's own schema, so every embedded object is extracted and
validated as the standalone object 5-Spot would create.

Every manifest under deploy/ and examples/ must also parse with no duplicate
mapping keys (PyYAML silently keeps the last, bug-012).

What is validated, each with `kubectl apply --dry-run=server --validate=strict`
(unknown fields are errors, and the CAPI / k0smotron webhooks run):

  * every YAML document under examples/;
  * every complete object (apiVersion + kind + metadata.name) in a fenced YAML
    block under docs/src/;
  * every bootstrapSpec / infrastructureSpec found in either, including in
    spec-only fragments, as a standalone object.

A file or fenced block containing the marker `validate-examples: expect-reject`
must be REJECTED; if the API server accepts it, the check fails. Anything that
cannot be validated on this cluster is listed with an explicit reason in
NOT_VALIDATED: nothing is skipped silently, because a check that skips reports
success while proving nothing (rules/testing.md).

Requires: a cluster with the 5-Spot CRDs and admission policy, Cluster API,
k0smotron, and the kubeadm and Docker providers (make e2e-k0smotron-up).
Usage: KUBECTL_CONTEXT=kind-5spot-e2e validate-examples.py
"""

from __future__ import annotations

import copy
import glob
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field

import yaml

EXPECT_REJECT_MARKER = "validate-examples: expect-reject"
# A docs block that deliberately shows only part of an object (the fields a page
# is about). Listed under NOT VALIDATED with the block's own reason, never
# passed silently. Its embedded bootstrapSpec / infrastructureSpec, if any, are
# still validated.
FRAGMENT_MARKER = "validate-examples: fragment"
VALIDATE_NS = "5spot-validate-examples"
FENCED_YAML = re.compile(r"```ya?ml\n(.*?)```", re.S)
EMBEDDED_KEYS = ("bootstrapSpec", "infrastructureSpec")

# Documents that are not API objects of a management cluster, or whose API is
# not installed by the validation environment. Keyed by (apiVersion group, kind);
# "*" matches any kind in the group. Every entry needs a reason a reviewer can
# check, because each one is a hole in this gate.
NOT_VALIDATED = {
    ("kind.x-k8s.io", "Cluster"): "kind's own cluster config file, read by the kind CLI, not an API object",
    ("k0s.k0sproject.io", "ClusterConfig"): "k0s configuration embedded in a K0smotronControlPlane, validated as part of that object",
    ("kustomize.config.k8s.io", "*"): "kustomize input, not an API object",
    ("infrastructure.cluster.x-k8s.io", "Metal3Machine"): "Metal3 provider not installed here; shape checked against CAPM3 v1.14.1's v1beta2 types by hand",
    ("infrastructure.cluster.x-k8s.io", "BareMetalMachine"): "illustrative placeholder kind in docs/src/advanced/capi-integration.md, not a real provider",
    ("infrastructure.cluster.x-k8s.io", "AWSMachine"): "AWS provider not installed here",
    ("monitoring.coreos.com", "*"): "Prometheus Operator CRDs not installed here",
}


@dataclass
class Item:
    source: str
    obj: dict
    expect_reject: bool
    note: str = ""
    fragment: str = ""


@dataclass
class Report:
    passed: list[str] = field(default_factory=list)
    failed: list[str] = field(default_factory=list)
    not_validated: list[str] = field(default_factory=list)


def group_of(api_version: str) -> str:
    return api_version.rsplit("/", 1)[0] if "/" in api_version else ""


def not_validated_reason(obj: dict) -> str | None:
    group = group_of(str(obj.get("apiVersion", "")))
    kind = str(obj.get("kind", ""))
    return NOT_VALIDATED.get((group, kind)) or NOT_VALIDATED.get((group, "*"))


def is_complete(doc: object) -> bool:
    return (
        isinstance(doc, dict)
        and isinstance(doc.get("apiVersion"), str)
        and isinstance(doc.get("kind"), str)
        and isinstance(doc.get("metadata"), dict)
        and bool(doc["metadata"].get("name"))
    )


def embedded_objects(doc: dict, parent: str) -> list[tuple[str, dict]]:
    """bootstrapSpec / infrastructureSpec as the standalone objects 5-Spot creates."""
    spec = doc.get("spec")
    if not isinstance(spec, dict):
        return []
    out = []
    for key in EMBEDDED_KEYS:
        emb = spec.get(key)
        if not isinstance(emb, dict) or "apiVersion" not in emb or "kind" not in emb:
            continue
        meta = copy.deepcopy(emb.get("metadata") or {})
        meta["name"] = f"{parent}-{key.lower()}"
        out.append((key, {
            "apiVersion": emb["apiVersion"],
            "kind": emb["kind"],
            "metadata": meta,
            "spec": copy.deepcopy(emb.get("spec") or {}),
        }))
    return out


class StrictLoader(yaml.SafeLoader):
    """A loader that rejects duplicate mapping keys.

    PyYAML's safe_load keeps the last of two equal keys without a word, which
    is how a manifest once shipped two `args:` on one container (bug-012).
    """


def _no_duplicate_keys(loader: yaml.SafeLoader, node: yaml.MappingNode, deep: bool = False):
    seen: dict[object, int] = {}
    for key_node, _ in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in seen:
            raise yaml.constructor.ConstructorError(
                None, None,
                f"duplicate key {key!r} (lines {seen[key] + 1} and {key_node.start_mark.line + 1})",
                key_node.start_mark,
            )
        seen[key] = key_node.start_mark.line
    return yaml.SafeLoader.construct_mapping(loader, node, deep)


StrictLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, _no_duplicate_keys)


def duplicate_key_failures(root: str) -> list[str]:
    """Every manifest under deploy/ and examples/ must parse with no duplicate keys."""
    failures = []
    patterns = ("deploy/**/*.yaml", "examples/**/*.yaml")
    for pattern in patterns:
        for path in sorted(glob.glob(os.path.join(root, pattern), recursive=True)):
            try:
                with open(path, encoding="utf-8") as handle:
                    list(yaml.load_all(handle, Loader=StrictLoader))
            except yaml.YAMLError as err:
                failures.append(f"{path}: {getattr(err, 'problem', err)}")
    return failures


def collect(root: str) -> list[Item]:
    items: list[Item] = []
    sources: list[tuple[str, str]] = []

    for path in sorted(glob.glob(os.path.join(root, "examples/**/*.yaml"), recursive=True)):
        sources.append((path, open(path, encoding="utf-8").read()))
    for path in sorted(glob.glob(os.path.join(root, "docs/src/**/*.md"), recursive=True)):
        text = open(path, encoding="utf-8").read()
        for n, block in enumerate(FENCED_YAML.findall(text), start=1):
            sources.append((f"{path} (yaml block {n})", block))

    for source, text in sources:
        expect_reject = EXPECT_REJECT_MARKER in text
        fragment = ""
        match = re.search(re.escape(FRAGMENT_MARKER) + r"\s*(\(.*?\))?", text)
        if match:
            fragment = match.group(1) or "(marked as a fragment)"
        is_example = source.endswith(".yaml")
        try:
            docs = [d for d in yaml.safe_load_all(text) if d is not None]
        except yaml.YAMLError as err:
            # An example that does not parse is a defect; a docs block may be
            # a deliberate fragment (with `...` elisions), so only examples fail.
            if is_example:
                items.append(Item(source, {}, False, note=f"YAML parse error: {err}"))
            continue
        for index, doc in enumerate(docs):
            label = f"{source}#{index}" if len(docs) > 1 else source
            complete = is_complete(doc)
            if complete:
                items.append(Item(label, doc, expect_reject, fragment=fragment))
            elif is_example and isinstance(doc, dict):
                if not_validated_reason(doc):
                    items.append(Item(label, doc, False))
                else:
                    items.append(Item(label, doc, False, note="example document is not a complete object"))
            if isinstance(doc, dict):
                parent = doc.get("metadata", {}).get("name", "fragment") if complete else "fragment"
                for key, emb in embedded_objects(doc, str(parent)):
                    # The embedded object of a deliberately rejected example is
                    # still expected to be valid on its own.
                    items.append(Item(f"{label} [{key}]", emb, False))
    return items


def dry_run(context: str, obj: dict) -> tuple[bool, str]:
    obj = copy.deepcopy(obj)
    meta = obj.setdefault("metadata", {})
    if obj.get("kind") != "Namespace":
        meta["namespace"] = VALIDATE_NS
    cmd = ["kubectl", "--context", context, "apply", "--dry-run=server", "--validate=strict", "-f", "-"]
    proc = subprocess.run(cmd, input=yaml.safe_dump(obj), capture_output=True, text=True, check=False)
    return proc.returncode == 0, (proc.stderr or proc.stdout).strip()


def main() -> int:
    context = os.environ.get("KUBECTL_CONTEXT", "")
    if not context:
        print("KUBECTL_CONTEXT is required (e.g. kind-5spot-e2e)", file=sys.stderr)
        return 2
    root = os.environ.get("REPO_ROOT", ".")

    subprocess.run(
        ["kubectl", "--context", context, "create", "namespace", VALIDATE_NS],
        capture_output=True, check=False,
    )

    report = Report()
    report.failed.extend(duplicate_key_failures(root))
    for item in collect(root):
        if item.note:
            report.failed.append(f"{item.source}: {item.note}")
            continue
        reason = not_validated_reason(item.obj) or (
            f"deliberate fragment {item.fragment}" if item.fragment else None
        )
        name = f"{item.obj.get('kind')}/{item.obj.get('metadata', {}).get('name')}"
        if reason:
            report.not_validated.append(f"{item.source}: {name}: {reason}")
            continue
        ok, message = dry_run(context, item.obj)
        if item.expect_reject:
            if ok:
                report.failed.append(f"{item.source}: {name}: ACCEPTED but marked '{EXPECT_REJECT_MARKER}'")
            else:
                report.passed.append(f"{item.source}: {name}: rejected as expected")
            continue
        if ok:
            report.passed.append(f"{item.source}: {name}")
        else:
            detail = " ".join(line.strip() for line in message.splitlines() if line.strip())
            report.failed.append(f"{item.source}: {name}: {detail or '(no output)'}")

    for line in report.passed:
        print(f"  ok    {line}")
    for line in report.not_validated:
        print(f"  NOT VALIDATED  {line}")
    for line in report.failed:
        print(f"  FAIL  {line}")
    print(
        f"\n{len(report.passed)} passed, {len(report.failed)} failed, "
        f"{len(report.not_validated)} not validated (each with a reason above)"
    )
    return 1 if report.failed else 0


if __name__ == "__main__":
    sys.exit(main())
