#!/usr/bin/env python3
"""Verify frozen root copies and run the existing local conformance checks.

Requires the Python jsonschema/referencing backend used by validate_package.py.
This validates canonical inputs only, not product codecs or a wave exit.
"""
import hashlib
import importlib.util
import json
from pathlib import Path
import socket
import ssl  # Load SSLSocket before socket.socket becomes the offline guard.
import sys
from collections import Counter

sys.dont_write_bytecode = True


def deny_network(*args, **kwargs):
    raise RuntimeError("canonical validation is offline")


socket.socket = deny_network
socket.create_connection = deny_network
socket.getaddrinfo = deny_network

root = Path(__file__).resolve().parents[1]
source = root / "build-control/orchestrator-architecture"
manifest = root / "build-control/canonical-inputs.sha256"
expected = {}
for line in manifest.read_text(encoding="utf-8").splitlines():
    digest, name = line.split("  ", 1)
    if name in expected or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
        raise SystemExit("invalid canonical checksum manifest")
    expected[name] = digest

if len(expected) != 75:
    raise SystemExit("canonical manifest must contain exactly 75 files")
for base in (root, source):
    actual = {
        path.relative_to(base).as_posix()
        for name in ("schemas", "fixtures")
        for path in (base / name).rglob("*")
        if path.is_file() and path.suffix == ".json"
    }
    if actual != set(expected):
        raise SystemExit("canonical path set mismatch")
    for name, digest in expected.items():
        path = base / name
        if path.is_symlink() or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            raise SystemExit("canonical byte checksum mismatch")

spec = importlib.util.spec_from_file_location(
    "frozen_validator", source / "evidence/validate_package.py"
)
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)
if not validator.SCHEMA_VALIDATOR_AVAILABLE:
    raise SystemExit("required schema-validation backend unavailable")

gates, counts = Counter(), {}
owners = validator.check_ownership(str(source), gates, counts)
schemas = validator.check_schemas(str(root), gates, counts, owners)
validator.check_graph_fixtures(str(root), gates, counts, schemas)
validator.check_admission_fixtures(str(root), gates, counts, schemas)
validator.check_host_capability_fixtures(str(root), gates, counts, schemas)
required_counts = {
    "SCHEMA_COUNT": 36,
    "GRAPH_FIXTURE_COUNT": 7,
    "GRAPH_NEGATIVE_FIXTURE_COUNT": 4,
    "ADMISSION_FIXTURE_COUNT": 7,
    "HOST_CAPABILITY_FIXTURE_COUNT": 8,
    "HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT": 13,
}
if any(counts.get(key) != value for key, value in required_counts.items()):
    raise SystemExit("canonical conformance count mismatch")
failures = {key: value for key, value in gates.items() if value}
print(json.dumps({
    "scope": "CANONICAL_INPUTS_ONLY",
    "offline": True,
    "files_checksum_verified": len(expected),
    "counts": counts,
    "failures": failures,
    "passed": not failures,
}, indent=2))
raise SystemExit(1 if failures else 0)
