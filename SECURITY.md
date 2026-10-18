# Security policy

Agent Guardian is in active development and is not yet a stable security product.

## Reporting a vulnerability

Please report suspected vulnerabilities through a private GitHub Security Advisory for this repository when that feature is enabled. Include affected version or commit, impact, and a minimal reproduction. Do not include real Wi-Fi credentials, tokens, or unrelated personal data.

If private reporting is unavailable, contact the repository owner through a verified account channel before publishing technical details. A public issue is not an appropriate place for an exploitable vulnerability.

## Scope

Security-sensitive areas include Windows power request lifetime, process identity and PID reuse, local configuration and logs, future local IPC, Wi-Fi recovery, and release/install integrity. The initial build has no elevated service, IPC endpoint, auto-update, or adapter mutation.

See the [threat model](docs/security/threat-model.md). Hardware tests and the dedicated pre-release review remain outstanding.
