# Canonical development inputs

Root `schemas/` and `fixtures/` are byte-identical copies of the 36 schemas
and 39 fixtures in the frozen architecture at
`f49d621ee510705939394f7df4996223a73fdcb7`. The originals remain normative.
`canonical-inputs.sha256` pins all 75 files, including their relative paths.
Do not edit a copy independently of an explicitly adopted contract change.
The historical non-JSON `schemas/SCHEMA_PLAN.md` is preserved; this manifest
covers the canonical JSON path set only.

Run `python3 build-control/validate_canonical_inputs.py` in an environment
with the existing architecture validator's Python `jsonschema` and
`referencing` dependencies. Missing backend, changed bytes, extra/missing
files and conformance failures are hard errors. The command denies network
access and uses only local schema resources. It validates schema structure,
graph topology/edge fixtures, admission fixtures and Host capability fixtures;
it does not claim product wire-codec, date-time format, integration, platform
or scenario coverage. It writes no source files.

State and Orchestration wire tests use exact `jsonschema = 0.58.6` as a dev
dependency with default features disabled: no HTTP or file resolver and no
TLS stack. Tests must register local schemas explicitly. Orchestration and
Workspace codecs receive the already-pinned serde/serde_json versions.
Existing dependency versions/checksums and production semantics are preserved.
No additional workspace member or test authority bypass is introduced.
