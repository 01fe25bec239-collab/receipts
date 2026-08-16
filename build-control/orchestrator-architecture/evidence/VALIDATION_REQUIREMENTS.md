# Validation requirements

These are freeze-validation tooling requirements only. They are not product or runtime dependencies.

- Python: CPython 3.10 or newer (the package was validated with the version recorded in the final attestation).
- Required library: `jsonschema>=4.18` (which installs `referencing`).
- Missing backend: `evidence/validate_package.py` reports `REQUIRED_VALIDATION_DEPENDENCIES_AVAILABLE = NO`, `SCHEMA_VALIDATION_EXECUTED = NO`, fails its dependency gates, and exits non-zero. `evidence/build_package.py` consequently fails.

macOS/Linux isolated environment:

```sh
python3 -m venv /tmp/orchestrator-validation-venv
/tmp/orchestrator-validation-venv/bin/python -m pip install 'jsonschema>=4.18'
/tmp/orchestrator-validation-venv/bin/python evidence/validate_sources.py
/tmp/orchestrator-validation-venv/bin/python evidence/run_regression.py
/tmp/orchestrator-validation-venv/bin/python evidence/validate_package.py
```

On Windows, replace the virtual-environment executable with `Scripts\\python.exe`.
