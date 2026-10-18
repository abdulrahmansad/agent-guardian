# Development status

Last updated: 2026-10-19

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
- [x] Added a separately authorized, one-shot saved-profile Wi-Fi reconnect using the Windows Native Wi-Fi API. It requires confirmed Wi-Fi disconnection, no active default route, and exactly one disconnected wireless interface; it uses the top Windows-preferred saved profile once, clears temporary profile-name copies, and uses a 20-second association-confirmation window. Synchronous Windows queries can extend total command duration. It never reads profile XML, retrieves credentials, restarts an adapter, or runs from the monitor. Controlled hardware behavior remains NOT TESTED.
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
- [x] Added `guardian install --sha256` and `guardian uninstall` for per-user executable install, upgrade, and removal. Installer tests on Windows used disposable paths (including spaces and non-ASCII characters) and covered checksum failure, upgrade, scoped removal, and the same-path/hard-link self-uninstall guard logic.
- [x] Installer tests and the full Rust workspace test suite passed on the Windows GNU/LLVM toolchain after the per-user install changes; formatting and Clippy with warnings denied also passed.
- [x] Rehashed the closed staged installer file before replacement and added a Windows file-identity guard plus tests for hard-link self-uninstall aliases.
- [x] Focused security review of installer/package paths confirmed the installer has no elevation or download path, verifies staged bytes, and limits replacement/removal to the fixed per-user executable; same-user path races and dynamic DLL loading remain documented limitations.
- [x] Added a deterministic local Windows x64 release packager, checksum sidecars, tests for reproducible output, architecture checks, and normal/delay-load import-dependency rejection, a release gate checklist, launch-copy drafts, and a safe read-only demo guide.
- [x] After the installer rehash and hard-link changes, `cargo fmt --all --check`, Clippy with warnings denied, and the Windows workspace tests passed (23 CLI/Windows tests and 38 core tests).
- [x] The docs link checker covered 19 Markdown files; official Agent Skills validation passed; local Skills CLI discovery found one skill; `cargo package --workspace --no-verify --allow-dirty` packaged both crates; `cargo audit` scanned 66 locked dependencies without advisories.
- [x] The Python package tests passed and verified deterministic ZIP output, executable architecture validation, and rejection of non-system DLLs in both normal and delay-load import tables.
- [x] Final local validation for this slice: formatting, Clippy, 23 CLI/Windows tests, 38 core tests, five release-packager tests, 22-file Markdown link checking, Agent Skills validation, local Skills CLI discovery, crate packaging, and `cargo audit` over 66 dependencies passed. The real MSVC build/package step is now in CI but has not run on the hosted workflow.
- [x] Added an explicit one-shot saved-profile Wi-Fi reconnect with no monitor-triggered recovery or adapter restarts. A read-only security review identified and fixed state revalidation, confirmation-window wording, result preservation after refresh failure, and profile-name privacy handling.
- [x] Revalidated this reconnect slice on Windows x64: `cargo fmt --all --check`, Clippy with warnings denied, all 38 core and 28 CLI/Windows tests, six package-release tests, 23-file Markdown link checking, Agent Skills validation, Skills CLI local discovery, crate packaging, and `cargo audit` over 66 dependencies passed.
- [x] Built and launched the local GNU/LLVM release executable; `guardian --version` succeeded. The packaging screen now recognizes `wlanapi.dll` as a Windows system dependency, while correctly rejecting the remaining `libunwind.dll` dependency.
- [x] Captured a bounded read-only monitor sample on Windows 11 x64: 60.66 seconds, 0.062 CPU seconds, and an 8.25 MiB sampled peak working set across 111 half-second samples. The monitor emitted initial/stopped aggregate snapshots and no stderr; this is one local sample, not a general performance claim.
- [x] Installed the skill with the Skills CLI into a disposable Codex project using copy mode, then listed it successfully. The temporary project was removed; no user profile or repository skill directory was changed.
- [x] Measured ten launches of the optimized local GNU/LLVM build's `--version` command: 21.04 ms median, 17.28–471.40 ms range. This is a local development-host sample, not a release performance claim.

## In progress

- [ ] Validate the CI dependency/security and documentation checks on a hosted Windows runner.
- [ ] Complete an install/upgrade/removal audit in a clean-user Windows VM using a release executable and independently trusted checksum.
- [ ] Measure longer foreground-monitor runs and repeat on other supported Windows configurations; the current one-minute sample is only a local baseline.

## Remaining work

- [ ] Integrate Wi-Fi recovery with event-driven monitor policy only after explicit user-intent handling, backoff/cancellation, typed incident reporting, and controlled hardware validation are designed and tested.
- [ ] Validate the one-shot Native Wi-Fi reconnection on an isolated adapter/profile and record association plus Internet-verification results; do not use the active workstation for disruptive testing.
- [ ] Validate published GitHub installation and Codex skill installation/uninstallation after the remote is configured and published.
- [ ] Build and exercise the actual MSVC release executable in a clean Windows environment; the import-table screening helper alone cannot prove runtime dependency closure. Do not publish artifacts without required authorization.
- [ ] Perform Windows power request lifecycle, crash cleanup, battery transition, display-off, and locked-session checks on representative hardware.
- [ ] Measure idle CPU, memory, probe frequency, startup latency, event latency, and bounded log growth before making performance claims.
- [ ] Complete the acceptance matrix, security review, release packaging/checksums, demo, and launch copy.
- [ ] Reassess public release readiness. Do not publish, push, tag, or release without the required authorization.

## Known failures and unverified assumptions

- `origin` points to `https://github.com/abdulrahmansad/agent-guardian.git`, but `git ls-remote origin HEAD` returned no refs. Remote contents, publication state, and permissions are not established.
- The system-required power request has compiled but has not been held in a real user session. Idle sleep, display-off, release, crash cleanup, Win+L, S3, and battery hardware behavior remain unverified.
- `powercfg /requests` requires an elevated shell on this host. `guardian doctor` reports the unavailable field and continues; `guardian status` returns the command's permission error.
- The default local network snapshot uses `GetAdaptersAddresses`. The `--probe` option has not been run against a live network; DNS resolver, route, ICMP, proxy behavior (including a real HTTP 407), and total runtime are not hardware-validated.
- The core recovery planner remains deterministic. The foreground monitor observes local interface/route changes only and cannot invoke recovery. The separate one-shot recovery command submits a saved-profile reconnect after conservative local-state checks, but there is no automatic recovery or adapter restart. The Native Wi-Fi API action, saved-profile behavior, connection verification, manual disconnect handling, and actual monitor notification delivery on a controlled network remain NOT TESTED.
- Installer and uninstaller file operations are implemented and tested only in disposable temporary directories. No public runtime artifact/checksum exists; clean-user installation, replacement of a running executable, and release-based removal remain untested. There is no updater, service/IPC, or agent runtime integration. Reports currently include power lease events only; network settings in the config are reserved and have no runtime effect.
- The installer rechecks and rehashes its closed staging file and compares Windows file identity to avoid self-uninstall through a hard link. A focused review recorded same-user concurrent filesystem tampering and ancestor-path races as residual limitations; the installer does not elevate, but these remain in the full pre-release security review. A supplied digest does not authenticate the artifact publisher.
- The CI workflow has not run on GitHub. The repository remote has no refs, so hosted CI and public Skills CLI install cannot be verified.
- The Rust 1.89 local cross-check could not link Windows GNU build scripts because the available LLVM-MinGW runtime lacks GCC support libraries; the workflow's 1.89 MSVC check is the authoritative remaining MSRV validation and has not run on GitHub.
- A monitor resource capture configured for 60 seconds exited successfully but the measuring process reported 18,494.7 seconds elapsed. The host may have been suspended or delayed; discard its CPU result and do not use its sparse memory samples as a baseline.
- The local optimized GNU/LLVM executable imports `libunwind.dll`; launching it requires the LLVM-MinGW runtime on `PATH`, and the current installer copies only `guardian.exe`. The release packager correctly rejects this binary. The MSVC artifact has not yet been built or exercised, and the static/delay-load import screen cannot detect dynamic DLL loading.
- No actual locked-session, Modern Standby/S3 sleep, battery, live Wi-Fi recovery, real-profile installer, clean-user installer, or real-agent integration acceptance run has been performed. Installer file operations were tested in disposable directories.

## Human-only or external gates

- Approval for destructive/disruptive network testing on a real adapter or active network.
- Physical-device validation across Windows 10/11, AC/battery, sleep models, locked session, and Wi-Fi hardware.
- Approval for public repository publication, tagging, release publication, or signed binaries if required.

## Next concrete action

Build and exercise the Windows x64 MSVC package on a suitable runner, then complete the hardware and hosted-CI acceptance gates. Do not describe the artifact as self-contained until clean-environment runtime checks pass. Keep automatic recovery, adapter restarts, and other unreviewed network mutations disabled; the one-shot saved-profile reconnect remains explicit and its real-hardware behavior is **NOT TESTED**.
