# Changelog

This project follows Semantic Versioning once public releases begin. Unreleased development behavior may change.

## Unreleased

- Redacted all `powercfg /requests` command output, followed and validated the Windows adapter linked list, and restricted PID-bound power leases to processes owned by the same user and session.
- Added a per-user Windows runtime install and upgrade command that verifies a supplied SHA-256 and stages the executable in `%LOCALAPPDATA%` without elevation, PATH changes, startup registration, or service installation.
- Added an uninstall command that removes only the Guardian executable and preserves configuration and logs. It refuses to remove the currently running executable.
- Added Windows installer tests for checksum failure, staged rehash, upgrade, uninstall scope, non-ASCII paths, and same-path/hard-link self-uninstall guard logic.
- Added deterministic local Windows x64 packaging with archive and executable checksums, plus release-gate and demo documentation. Generated packages are labeled local and are not official releases.
- Added the initial Rust workspace and agent-independent configuration, process lease, connectivity classification, recovery decision, and incident report types.
- Connected bounded configuration loading to power-lease limits and local journal size/retention.
- Added an experimental foreground Windows power-request CLI and read-only power diagnostics.
- Added a privacy-limited Windows adapter snapshot, bounded battery-transition policy, exponential retry backoff, and a hard rolling adapter-restart budget in the core.
- Added one-shot DNS, route, gateway, and web evidence with conservative classification, including explicit proxy-authentication rejection handling.
- Added an opt-in foreground local network monitor using Windows interface/route notifications and bounded fallback snapshots; it performs no probes, recovery, or network changes.
- Added a serialized, rotating local JSONL event journal for power leases plus human and JSON session reports that preserve incomplete-session status.
- Added the portable Agent Skill package and initial architecture, threat model, and acceptance matrix.
- VPN/captive-portal/upstream monitoring, network recovery, hardware notification validation, published runtime artifacts, and end-to-end agent integration remain incomplete.
