# Contributing

Thanks for helping improve Agent Guardian. The project is early-stage, and safety evidence matters as much as code.

## Before opening a change

- Read [the architecture](docs/architecture/overview.md), [threat model](docs/security/threat-model.md), and [acceptance matrix](docs/testing/acceptance.md).
- Keep the runtime least-privileged and agent-independent.
- Separate evidence collection from recovery decisions. Unknown connectivity must not trigger disruptive recovery.
- Do not add secret collection, telemetry, arbitrary shell execution, power-plan changes, or adapter changes without a reviewed design.
- Do not run network-disruptive tests on a user's active workstation. Use deterministic simulation unless the maintainer explicitly approves a controlled test.

## Development checks

Run these commands before submitting a code change:

    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace --all-targets
    python scripts/check-doc-links.py

Update documentation and docs/STATUS.md when behavior, decisions, tests, risks, or release blockers change. Record hardware scenarios as NOT TESTED unless the scenario was actually performed.

## Pull requests

Explain the user-visible behavior, safety impact, tests performed, and remaining limitations. Do not label a change release-ready based only on unit tests.

Contributors are expected to follow the project [Code of Conduct](CODE_OF_CONDUCT.md).
