# Changelog

This project follows Semantic Versioning once public releases begin. Unreleased development behavior may change.

## Unreleased

- Added the initial Rust workspace and agent-independent configuration, process lease, connectivity classification, recovery decision, and incident report types.
- Connected bounded configuration loading to power-lease limits and local journal size/retention.
- Added an experimental foreground Windows power-request CLI and read-only power diagnostics.
- Added a privacy-limited Windows adapter snapshot, bounded battery-transition policy, exponential retry backoff, and a hard rolling adapter-restart budget in the core.
- Added a serialized, rotating local JSONL event journal for power leases plus human and JSON session reports that preserve incomplete-session status.
- Added the portable Agent Skill package and initial architecture, threat model, and acceptance matrix.
- DNS/VPN/captive-portal/upstream observation, network recovery, hardware validation, install/uninstall, and agent integration remain incomplete.
