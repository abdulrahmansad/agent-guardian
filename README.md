# Agent Guardian

**Bounded Windows power leases and privacy-limited connectivity diagnostics.**

Agent Guardian is an early-stage Windows project for protecting long-running local AI coding sessions from idle sleep and transient connectivity failures. It combines a portable Agent Skill, an independent Rust runtime, and a conservative Windows integration.

The repository currently contains an experimental foreground power-lease implementation, bounded local session event storage, JSON/human session reports, a read-only local network snapshot, a bounded foreground monitor for local IP-interface and route changes, an explicitly requested one-shot connectivity check, an explicitly authorized one-shot saved-profile Wi-Fi reconnect, deterministic policy core, and a checksum-verified per-user executable installer. Automatic recovery, a published runtime package, and end-to-end Codex installation have not been completed. Do not rely on this development build for unattended production work.

## The use case

You start a long-running coding task in Codex and step away. Windows may enter idle sleep before the task finishes, and Wi-Fi can disconnect temporarily. Guardian is designed to request that Windows keep the system awake while leaving the display free to turn off. Its opt-in foreground monitor observes local IP-interface and route changes. Its one-shot network diagnostic can combine local adapter state, DNS lookup results, an active default route, a gateway echo response, and two fixed web checks. A separately authorized command can request one reconnection through the highest-priority saved Wi-Fi profile when a single Wi-Fi interface is confirmed disconnected and no default route is active. Automatic recovery remains under development.

Guardian cannot keep a remote model generating after its stream has ended or restore an unavailable ISP, router, or remote service.

## Initial goals

- Keep Windows from entering idle sleep during a user-authorized, bounded coding session while allowing the display to turn off.
- Detect and classify connectivity changes using multiple local and remote signals before suggesting any recovery.
- Give Codex concise, accurate Guardian state and safe retry guidance.
- Make setup, diagnostics, reports, privacy boundaries, and removal understandable to other developers.

These are project goals. The status section below identifies what the current development build actually implements.

## Project status

This repository is under active development. The current Windows CLI can request a bounded system-required power lease, inspect Windows power diagnostics, refuse battery operation unless explicitly enabled, and install or upgrade a per-user executable after checking its SHA-256. The core includes validated TOML configuration types, PID plus process-creation identity leases, conservative connectivity classification, and pure recovery decisions.

The default network command reports a privacy-limited local adapter snapshot and sends no probes. `guardian monitor` is a separate, explicit foreground listener for local IP-interface and route changes. It runs for at most 60 minutes, takes a local fallback snapshot every five minutes by default, and sends no DNS, HTTP, or ICMP probes. `guardian network --probe` is an explicit, one-shot opt-in that resolves two fixed names, reads active default-route state, attempts a bounded ICMP echo to each usable gateway family, checks two fixed public endpoints, and shows a preliminary deterministic assessment. The assessment can report `HEALTHY` only when DNS, route, gateway, and both independent endpoint checks all succeed. `guardian network --reconnect-saved-wifi` is a separate one-shot action: it requires a confirmed Wi-Fi disconnect, a confirmed absence of an active default route, and exactly one disconnected wireless interface; it makes at most one saved-profile connection request and uses a 20-second Wi-Fi association confirmation window. Synchronous Windows queries can extend total command duration beyond that window. It does not confirm internet access. Automatic recovery, VPN-state classification, and persistent/background monitoring are not implemented. There is no service, IPC endpoint, autostart entry, or updater. The per-user installer is implemented, but no runtime release artifact or trusted release checksum is available yet.

See [development status](docs/STATUS.md), [acceptance matrix](docs/testing/acceptance.md), [threat model](docs/security/threat-model.md), and the [local release preparation checklist](docs/release/README.md). A read-only [development demo](demos/unattended-session.md) is also available.

## Features

- **Power Guardian:** Experimental foreground system-required power lease. The display is not requested to stay on. User-initiated sleep, shutdown, restart, critical battery policy, Windows policy, and device behavior remain in control.
- **Network Guardian:** Experimental local observation reports Wi-Fi adapter state, gateway presence, and a tunnel-type interface hint. `guardian monitor` listens for local IP-interface and route changes, refreshing an aggregate snapshot on events and at a bounded fallback interval. It is foreground-only, expires within 60 minutes, sends no probes, logs no events, and performs no recovery. `guardian network --probe` adds two fixed DNS lookups, active default-route inspection, bounded gateway ICMP checks, two one-shot public connectivity checks, and a preliminary core classification. `guardian network --reconnect-saved-wifi` explicitly requests one reconnection through the first saved profile in Windows preference order, only when local evidence confirms a disconnected Wi-Fi interface and no active default route; it refuses multiple or ambiguous interfaces, does not restart adapters, and does not expose profile names or credentials. It uses a 20-second association confirmation window; synchronous Windows queries can extend total command duration. Wi-Fi association does not prove internet access. Missing probe evidence prevents a `HEALTHY` result; an absent ICMP reply remains unknown because networks often filter ping. It does not confirm VPN state. Recovery remains manual and hardware behavior is **NOT TESTED**.
- **Agent integration:** The Agent Skill package passes metadata validation and was copied and listed for Codex by the Skills CLI in a disposable project. Remote installation and end-to-end runtime behavior have not yet been validated.
- **Incident reports:** Power lease events and the outcome of an explicitly requested saved-profile reconnect are stored as typed, bounded local JSONL events. Reconnect records contain an event UUID, timestamp, and redacted result category; monitor and probe observations are not journaled. The CLI can show events or export a session report. Interrupted sessions remain marked incomplete.

## Quick start

Install the Agent Skill with the standard Skills CLI:

    npx skills add abdulrahmansad/agent-guardian

This installs only the Agent Skill. If the CLI asks, select Codex. You can select it without prompts with:

    npx skills add abdulrahmansad/agent-guardian --skill agent-guardian --agent codex

It does not install the Windows runtime or grant administrator access. The GitHub repository has no published runtime artifact yet, and remote installation has not been validated because the configured remote has no published refs.

To build and run the development CLI from a Rust environment:

    cargo run -p guardian-cli -- doctor
    cargo run -p guardian-cli -- start --ttl-minutes 60
    cargo run -p guardian-cli -- start --config config.example.toml --ttl-minutes 30

The start command runs in the foreground and binds the lease to Guardian's own process. Press Ctrl+C to end it. The request also expires after the configured duration. Start on AC power for a useful unattended session. On battery, the command refuses by default; the allow-battery option permits a lease capped at five minutes.

To bind a lease to an existing process owned by the same Windows user and running in the same session as Guardian:

    cargo run -p guardian-cli -- start --pid 1234 --ttl-minutes 60

Replace 1234 with the process ID. Guardian refuses targets owned by another Windows user or session. The lease ends when that process exits or the duration expires. Confirm the selected process and power source before starting a lease.

## Supported platforms

| Platform | Status |
|---|---|
| Windows 11 x64 | Development target; source and installer file operations are tested on Windows 11 x64. Power behavior and locked-session behavior have not been hardware-validated. |
| Windows 10 x64 | Not tested |
| Windows ARM64 | Not tested |
| macOS / Linux | Core design is platform-independent; runtime is unsupported |

Windows sleep behavior depends on device firmware, power policy, and sleep model. Modern Standby can limit requests on battery. Use the doctor command and powercfg /a to inspect the local system.

## Supported agents

| Agent | Status |
|---|---|
| OpenAI Codex | Experimental. The skill validates and is discoverable locally; remote installation and end-to-end runtime workflows remain unverified. |
| Other coding agents | Planned |

## Installation and permissions

The Agent Skill and the Windows runtime are separate installations. Skills CLI installation copies skill guidance into the selected agent's project or user skill directory. The Windows installer accepts the SHA-256 of the executable being run and copies it to `%LOCALAPPDATA%\Programs\AgentGuardian\guardian.exe`. It does not require elevation or change PATH, startup settings, power plans, services, configuration, or logs. Upgrades use the same command with a new verified executable. Close running Guardian instances before upgrading.

No runtime release has been published yet. The current local GNU/LLVM build depends on `libunwind.dll`; the installer copies only `guardian.exe`, so do not install that development build as a standalone runtime. Use `cargo run` for local development. A Windows x64 MSVC build can be installed after its runtime dependencies have been checked on a clean Windows environment and its digest computed with PowerShell:

    $exe = (Resolve-Path '.\target\x86_64-pc-windows-msvc\release\guardian.exe').Path
    $sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    & $exe install --sha256 $sha256

This local hash confirms the bytes copied into the install directory; it does not verify who built the executable. For a future release, use the checksum distributed with the release and verify that release through a trusted source. See the [runtime installation guide](docs/development/installation.md).

No service, SYSTEM process, machine-wide power-plan change, firewall change, DNS change, or adapter restart is installed by the current build.

## Usage

The doctor command shows available sleep states, power source, and battery percentage where Windows reports it. It reports whether the `powercfg /requests` query succeeded, but discards requestor names, process IDs, and paths. This device requires an elevated shell for that query; doctor reports the unavailable field without failing the other checks. The status command runs the same query and prints only a generic success message with requestor details redacted; command output is suppressed even on failure. The network command reports local Wi-Fi adapter state, gateway presence on any adapter and on operational adapters, operational adapter counts, and whether Windows reports a tunnel-type interface. The tunnel flag is only a hint and does not confirm that a VPN is active. The command does not expose adapter names or addresses. Use `guardian --json network` for machine-readable output. Run `guardian monitor` to watch local interface and route metadata for up to 60 minutes (default), with a five-minute fallback snapshot and early Ctrl+C stop. `guardian --json monitor` emits newline-delimited JSON snapshots. The monitor sends no probes and performs no recovery. The start command holds a foreground lease and clears its request when its target exits or its lease expires. Use Ctrl+C in the owning terminal to stop it.

By default, the network command reports local Wi-Fi adapter state, gateway presence, and a tunnel-type hint only. `guardian monitor` observes local IP-interface and route-table changes using Windows notifications, with a periodic local snapshot as fallback. It ends after 60 minutes by default (maximum 60), or on Ctrl+C; `--interval-seconds` accepts 30 through 3600 (default 300). It sends no DNS, HTTP, or ICMP probes and makes no network or power changes. It reports only aggregate adapter flags/counts and default-route presence, writes no event log, and cannot determine internet health. `guardian network --probe` additionally performs system DNS lookups for the two fixed endpoint names, reads active default-route metadata, sends at most one 750 ms ICMP echo to an eligible gateway for each address family, and checks two fixed web endpoints: Microsoft’s NCSI HTTP endpoint, which must return the expected `Microsoft Connect Test` text, and a Cloudflare HTTPS endpoint, which must return HTTP 200. Windows automatic proxy settings are used for the web checks, while server and proxy authentication, redirects, and cookies are disabled; a visible `407 Proxy Authentication Required` response is classified as `PROXY_RESTRICTED`. Other proxy setup or transport failures remain ambiguous. Guardian displays only boolean summaries. Gateway addresses are used in memory for route checks and ICMP and are not displayed, logged, or persisted. DNS results, route metadata, gateway response, and endpoint outcomes feed a preliminary deterministic assessment. If ICMP is filtered or any required signal is unknown, Guardian will not claim `HEALTHY`. Response text is not logged or persisted. WinHTTP receives 2-second per-stage timeout settings; DNS has a 2-second timeout and each gateway echo has a 750 ms timeout. Windows TCP retries or proxy discovery can extend total time. A one-shot probe does not start the monitor or a recovery action. Adapter recovery is not implemented. See [network probe details](docs/development/network-probes.md) and [monitor details](docs/development/network-monitor.md).

For one explicitly authorized Wi-Fi reconnect attempt, run `guardian network --reconnect-saved-wifi`. This can change the active Wi-Fi connection; it is refused unless Wi-Fi disconnection, no active default route, and one disconnected wireless interface are confirmed. Guardian uses one existing profile, omits its name from output, and waits up to 20 seconds for Wi-Fi association. This does not prove Internet access. The monitor does not invoke recovery automatically. See [saved Wi-Fi recovery details](docs/development/network-recovery.md).

Use `guardian logs` to list recorded power lease events and explicit reconnect outcomes. `guardian --json logs` returns machine-readable event data. `guardian report` summarizes the latest session and includes reconnect outcomes whose timestamps fall within that session's recorded interval (or up to report time if it is incomplete). `guardian report --output report.json` writes a JSON file without overwriting an existing path. An incomplete session has no reported protected duration.

## Architecture

    Agent Skill / Codex guidance
              |
              v
         guardian CLI
              |
              v
     Agent-independent Rust core
        |                 |
        v                 v
    Windows APIs    Local config, logs, reports

The initial architecture keeps the runtime in the interactive user session. The power layer uses a scoped system-required request. Network detection and recovery policy are separate, so uncertainty can result in waiting or user guidance without changing an adapter.

See [architecture and decisions](docs/architecture/overview.md), including [one-shot network probe details](docs/development/network-probes.md).

The [configuration example](config.example.toml) documents validated defaults. `guardian start`, `guardian logs`, and `guardian report` read `%LOCALAPPDATA%\AgentGuardian\config.toml` when present, or accept `--config PATH`. `guardian network --reconnect-saved-wifi` also accepts `--config PATH` to select the local journal's size and retention limits. The CLI does not create a config file. Configuration is limited to 64 KiB; network-monitoring settings are reserved and do not enable monitoring in this build.

## Privacy and security

Guardian has no account requirement or mandatory telemetry. The default CLI does not send network probes. When `guardian network --probe` is used, it makes two fixed web requests through Windows automatic proxy settings, resolves their fixed hostnames through the Windows DNS namespace, reads the local route table, and may send a bounded ICMP echo to a selected default gateway. The Microsoft NCSI check uses plain HTTP; at most 64 response bytes are compared in memory and discarded. The Cloudflare check uses HTTPS and reads only the status code. Gateway IP addresses are used transiently for route and ICMP checks, then discarded; Guardian does not display or persist them. Probe results and response bodies are not written to the journal. Power-request diagnostics discard Windows command output because it can contain requestor names, process IDs, and paths. Guardian does not persist Wi-Fi passwords, SSIDs, adapter names, IP addresses, API keys, tokens, or traffic. Power lease events and explicitly requested reconnect outcomes are kept in separate, bounded local JSONL files under `%LOCALAPPDATA%\AgentGuardian`; reconnect records contain an event UUID, timestamp, and typed result category.

The initial design avoids privilege elevation and arbitrary shell commands. The installer relies on a user-supplied SHA-256 and does not authenticate publisher identity. See the [threat model](docs/security/threat-model.md) before relying on future recovery features. Report vulnerabilities through a private GitHub Security Advisory when the repository enables that feature.

## Limitations

- A power request asks Windows to prevent idle sleep. Windows policy, user actions, critical battery handling, hardware, or sleep model can end or ignore it.
- Modern Standby on battery limits ordinary power requests. The development CLI caps battery leases at five minutes.
- Power source changes are checked every ten seconds while the foreground lease is active; an unsupported battery transition can therefore take up to one check interval to release.
- Win+L and screen-off behavior have not been validated on physical hardware.
- The `monitor` command observes local interface and route metadata only. It does not test internet, DNS, VPN, or remote-service health; the bounded foreground monitor is not a background service. The `--probe` option separately runs only on explicit request and uses DNS and ICMP in addition to two fixed web checks. ICMP filtering can leave gateway reachability unknown.
- Automatic network recovery and adapter restarts are not implemented. The explicitly authorized one-shot saved-profile reconnect has not been tested on a controlled physical network.
- Guardian cannot repair router, ISP, VPN-provider, DNS-provider, GitHub, OpenAI, npm, or other remote-service outages.
- Guardian cannot restore a terminated Codex model stream or safely replay non-idempotent operations.
- Automatic Wi-Fi reconnection and adapter recovery are not implemented.

## FAQ

**Does installing the skill install a Windows service?** No. The skill is guidance for a coding agent. Runtime installation is separate; the runtime installer copies one executable into the current user's profile and installs no service.

**Will the screen stay on?** Guardian requests system wakefulness only. It does not request the display to stay on.

**Can I close the lid or press Sleep and expect Guardian to override that?** No. Explicit user sleep actions and lid policy remain under Windows control.

**Will Guardian fix a failed OpenAI or GitHub endpoint by restarting Wi-Fi?** No. The core recovery policy routes remote-only failures to user notification, and live recovery is not implemented.

**Can Codex keep generating while offline?** Guardian cannot keep a disconnected model stream alive. An agent can continue local work that does not require the remote model or service.

## Uninstallation

Run `guardian uninstall` from a different Guardian executable, such as a local development build or a downloaded release file, after closing other Guardian instances. The command removes only `%LOCALAPPDATA%\Programs\AgentGuardian\guardian.exe` and removes that directory only if empty. It refuses to delete the executable that is currently running. Configuration, logs, and reports under `%LOCALAPPDATA%\AgentGuardian` are preserved. Remove that directory separately only if you also want to delete Guardian's local history. Remove the Agent Skill separately using the Skills CLI's removal command for the selected agent.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md), the [architecture decisions](docs/architecture/overview.md), and [acceptance matrix](docs/testing/acceptance.md). Network-disruptive testing must use an isolated test device and requires explicit approval.

## Development

Install Rust 1.89 or newer with Cargo, rustfmt, and Clippy. The current workspace uses Rust edition 2024.

    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace --all-targets
    python scripts/check-doc-links.py
    cargo run -p guardian-cli -- doctor

Windows power and hardware behavior require Windows validation. Power behavior, live recovery, and locked-session acceptance scenarios remain untested. See the [acceptance matrix](docs/testing/acceptance.md). Use `cargo run -p guardian-cli -- --help` to inspect the current CLI.

## License

MIT. See [LICENSE](LICENSE).
