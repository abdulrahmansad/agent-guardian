# Development status

Last updated: 2026-10-18

This is the source-of-truth checklist for the Agent Guardian master goal. Automated policy tests and read-only command smoke tests do not establish physical Windows power behavior or safe live network recovery.

## Completed

- [x] Created and committed the initial README, Rust-oriented `.gitignore`, and MIT license (`83e48e6`).
- [x] Inspected the empty repository, remote configuration, Git identity, and available Windows development commands.
- [x] Confirmed the development host is Windows 11 Home, build 26200, x64.
- [x] Installed Rust 1.99.1, rustfmt, Clippy, and checksum-verified LLVM-MinGW in the user profile.
- [x] Researched Windows power requests, Modern Standby limits, `powercfg`, Agent Skills, and Vercel Skills CLI using official documentation.
- [x] Validated the local skill with `skills-ref`; the official validator reports it as valid.
- [x] Ran Vercel Skills CLI local discovery; it found exactly one skill named `agent-guardian`.
- [x] Added the Rust workspace, validated configuration and process-bound lease core, conservative connectivity classifier, retry backoff, rolling restart budget, and report model.
- [x] Added the Windows foreground power lease with target process identity, bounded TTL, battery opt-in/cap, battery-transition handling, display-off eligibility, and system-directory `powercfg` lookup.
- [x] Added a Windows local network snapshot that reports Wi-Fi adapter operational state and gateway presence without exposing adapter names, SSIDs, addresses, or sending probes.
- [x] Added privacy-limited local summary flags for tunnel-type interfaces; the signal does not claim that a VPN is active.
- [x] Added `guardian network --probe` as an explicit one-shot diagnostic with two DNS lookups, active default-route inspection, bounded ICMP gateway checks, and two fixed web checks. Redirects, server/proxy authentication, cookies, and keep-alive are disabled; results are not logged. Live network behavior is not tested.
- [x] Integrated DNS, route, gateway, endpoint, captive-portal, and local adapter evidence with the deterministic classifier. A `HEALTHY` result requires positive DNS, default route, gateway echo, and two independent endpoint checks; absent ICMP replies remain unknown.
- [x] Recognized an explicit HTTP `407 Proxy Authentication Required` response as `PROXY_RESTRICTED`, which leads to user notification without retries or network changes.
- [x] Added `guardian monitor`, a foreground listener for local IP-interface and route changes, with a bounded fallback snapshot and a 60-minute maximum duration. It performs no external probes, network/power changes, recovery, or event logging.
- [x] Added a locked, bounded local JSONL journal for typed power lease events, with rotation, retention, and human/JSON event output.
- [x] Added latest-session reports that derive duration only from recorded start/release events and leave interrupted sessions incomplete.
- [x] Connected validated configuration loading to power-lease defaults/maximums and bounded journal size/retention. Config files are read-only, optional, and limited to 64 KiB.
- [x] Added an integration test that applies a 1 MiB configured journal limit, verifies rotation, and removes a file older than the configured one-day retention in a disposable directory.
- [x] Added architecture, threat model, acceptance matrix, contributor/security docs, changelog, issue templates, local documentation-link validation, and least-privilege Windows CI workflow.
- [x] `cargo fmt --all` completed successfully.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` completed successfully using the Windows GNU/LLVM toolchain.
- [x] `cargo test --workspace --all-targets -j 1` passed 30 core tests and 7 CLI/Windows-probe simulation tests on the Windows GNU/LLVM toolchain after local adapter, journal, and opt-in probe changes.
- [x] `guardian doctor` ran on this Windows host and reported S3 sleep support, current power source/battery status, and that this account needs elevation for `powercfg /requests`.
- [x] `guardian network` ran on this Windows host and returned only summary booleans/counts. The JSON smoke test showed no adapter names or addresses.
- [x] `guardian logs` and `guardian report` passed a CLI smoke test against a disposable JSONL fixture; the JSON report matched the recorded 2-second interval. The output file did not overwrite an existing path.
- [x] `python scripts/check-doc-links.py` checked all Markdown files; all local file references resolved.
- [x] Official `skills-ref` validation at pinned source revision `69ef37e9424c0a7ea9dd2293b559e43ec8176379` reports the skill as valid.
- [x] Vercel Skills CLI local discovery found exactly one skill, `agent-guardian`; this did not install it.
- [x] `cargo audit` using the checksum-verified official `cargo-audit 0.22.2` Windows binary scanned 58 locked dependencies with no advisories.
- [x] Re-ran `cargo clippy --workspace --all-targets -- -D warnings` after local adapter, journal, and opt-in probe changes; it passed.
- [x] Re-ran `cargo fmt --all --check`, `python scripts/check-doc-links.py`, official Agent Skills validation, `cargo test`, and `cargo audit` after local adapter, journal, and opt-in probe changes; all passed.
- [x] Ran a Windows `guardian --json network` smoke check after the change. It returned only booleans/counts, including `tunnel_adapter_detected`, with probes and network changes reported false.
- [x] `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` passed after classifier integration.
- [x] `cargo test --workspace --all-targets -j 1` passed 35 core tests and 7 CLI/Windows-probe simulation tests after classifier integration.
- [x] `python scripts/check-doc-links.py` checked all 17 Markdown files; official `skills-ref` validation, Vercel Skills CLI local discovery, and `cargo audit` over 58 locked dependencies passed after classifier integration.
- [x] Current network signal and proxy-classification slice: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --all-targets -j 1` passed (38 core tests, 10 CLI tests); documentation links, official skill validation, local Skills CLI discovery, and `cargo audit` over 58 locked dependencies passed.
- [x] Ran the Windows local-only `guardian --json network` smoke check. It reported `external_probes_attempted: false`, `internet_connectivity_tested: false`, and `network_settings_changed: false`; no live `--probe` was run.
- [x] After adding the foreground monitor: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --all-targets -j 1` passed (38 core tests, 13 CLI tests). The documentation checker covered 18 Markdown files, official skill validation passed, local Skills CLI discovery found one skill, and `cargo audit` scanned 58 dependencies without advisories.
- [x] Ran `guardian --json monitor --duration-minutes 1 --interval-seconds 30` on Windows 11 x64. It registered both notifications, emitted an aggregate-only initial snapshot, completed its bounded run, and emitted a stopped record. No local state change occurred during the run; actual event delivery remains unverified.

## In progress

- [ ] Build verified runtime installation, upgrade, and removal flows without unnecessary elevation.
- [ ] Validate the CI dependency/security and documentation checks on a hosted Windows runner.

## Remaining work

- [ ] Implement any network recovery only after evidence, explicit authorization, retry budgets, and controlled hardware testing are in place.
- [ ] Validate published GitHub installation and Codex skill installation/uninstallation after the remote is configured and published.
- [ ] Perform Windows power request lifecycle, crash cleanup, battery transition, display-off, and locked-session checks on representative hardware.
- [ ] Measure idle CPU, memory, probe frequency, startup latency, event latency, and bounded log growth before making performance claims.
- [ ] Complete the acceptance matrix, security review, release packaging/checksums, demo, and launch copy.
- [ ] Reassess public release readiness. Do not publish, push, tag, or release without the required authorization.

## Known failures and unverified assumptions

- `origin` points to `https://github.com/abdulrahmansad/agent-guardian.git`, but `git ls-remote origin HEAD` returned no refs. Remote contents, publication state, and permissions are not established.
- The system-required power request has compiled but has not been held in a real user session. Idle sleep, display-off, release, crash cleanup, Win+L, S3, and battery hardware behavior remain unverified.
- `powercfg /requests` requires an elevated shell on this host. `guardian doctor` reports the unavailable field and continues; `guardian status` returns the command's permission error.
- The default local network snapshot uses `GetAdaptersAddresses`. The `--probe` option has not been run against a live network; DNS resolver, route, ICMP, proxy behavior (including a real HTTP 407), and total runtime are not hardware-validated.
- The core recovery planner is deterministic policy only. DNS, route, gateway, and web outcomes feed the classifier, but there is no reconnection or adapter restart. The foreground monitor observes local interface/route changes only; actual notification delivery on a controlled network has not been tested.
- Installer, updater, service/IPC, and agent runtime integration are not implemented. Reports currently include power lease events only; network settings in the config are reserved and have no runtime effect.
- The CI workflow has not run on GitHub. The repository remote has no refs, so hosted CI and public Skills CLI install cannot be verified.
- No actual locked-session, Modern Standby/S3 sleep, battery, Wi-Fi recovery, installer, or real-agent integration acceptance run has been performed.

## Human-only or external gates

- Approval for destructive/disruptive network testing on a real adapter or active network.
- Physical-device validation across Windows 10/11, AC/battery, sleep models, locked session, and Wi-Fi hardware.
- Approval for public repository publication, tagging, release publication, or signed binaries if required.

## Next concrete action

Build verified per-user runtime installation, upgrade, and removal flows. Keep network-changing behavior disabled until its evidence and authorization path are complete.
