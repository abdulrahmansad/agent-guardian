# Agent Guardian

**Bounded Windows power leases and privacy-limited connectivity diagnostics.**

Agent Guardian is an early-stage Windows project for protecting long-running local AI coding sessions from idle sleep and transient connectivity failures. It combines a portable Agent Skill, an independent Rust runtime, and a conservative Windows integration.

The repository contains an experimental Windows runtime with bounded foreground or detached power leases, a same-user stop command, a local network monitor, explicit one-shot diagnostics, optional bounded saved-profile Wi-Fi reconnection, local event storage and reports, and a per-user installer. Detached start waits for the worker to confirm that its power request is active (and, when requested, that its monitor initialized). The Agent Skill is publicly discoverable and its Codex project installation has been verified in a disposable project. One approved live test of the previous Wi-Fi recovery candidate failed to produce a Guardian reconnect attempt; a classifier fix now has simulated and hosted CI coverage but has not been retested on hardware. Power behavior, an official runtime release, and end-to-end agent runtime integration remain unverified. Do not rely on this development build for unattended production work.

## The use case

You start a long-running coding task in Codex and step away. Windows may enter idle sleep before the task finishes, and Wi-Fi can disconnect temporarily. Guardian can request that Windows keep the system awake while leaving the display free to turn off. Its optional monitor observes local IP-interface and route changes. Automatic saved-profile reconnection can be enabled explicitly after sustained local Wi-Fi loss. In the one live test of the previous candidate, Guardian detected the disconnect but did not attempt recovery; the current classifier fix still needs a live retest.

Guardian cannot keep a remote model generating after its stream has ended or restore an unavailable ISP, router, or remote service.

## Initial goals

- Keep Windows from entering idle sleep during a user-authorized, bounded coding session while allowing the display to turn off.
- Detect and classify connectivity changes using multiple local and remote signals before suggesting any recovery.
- Give Codex concise, accurate Guardian state and safe retry guidance.
- Make setup, diagnostics, reports, privacy boundaries, and removal understandable to other developers.

These are project goals. The status section below identifies what the current development build actually implements.

## Project status

This repository is under active development. The current Windows CLI can request a bounded system-required power lease, stop active leases in the current user session, inspect Windows power diagnostics, refuse battery operation unless explicitly enabled, and install or upgrade a per-user executable after checking its SHA-256. The core includes validated TOML configuration types, PID plus process-creation identity leases, conservative connectivity classification, and pure recovery decisions.

The default network command reports a privacy-limited local adapter snapshot and sends no probes. `guardian monitor` listens for local IP-interface and route changes for up to 24 hours, taking a fallback snapshot every five minutes by default. `--auto-reconnect-saved-wifi` explicitly enables a bounded saved-profile reconnect when Wi-Fi stays disconnected for 10 seconds and no default route exists; it uses the last connected profile retained in memory, refuses to guess if that profile is unavailable, allows at most three consecutive attempts with backoff, and never restarts an adapter. `guardian network --probe` is a separate one-shot opt-in that checks fixed DNS names, the default route, gateway reachability, and two public endpoints. `guardian network --reconnect-saved-wifi` requests one saved-profile connection after the same local Wi-Fi checks. Neither recovery path confirms Internet access. One live test of the previous candidate failed to reconnect before an independent fallback request; the current classifier fix has not been verified on hardware. VPN-state detection is not implemented. There is no service, named pipe, autostart entry, or updater. Background work is a bounded same-user process stopped by `guardian stop`. The per-user installer is implemented, but no runtime release artifact or trusted release checksum is available yet.

See [development status](docs/STATUS.md), [acceptance matrix](docs/testing/acceptance.md), [threat model](docs/security/threat-model.md), and the [local release preparation checklist](docs/release/README.md). A read-only [development demo](demos/unattended-session.md) is also available.

## Features

- **Power Guardian:** Experimental bounded system-required power lease, available in the foreground or detached with `guardian start --background`. The display is not requested to stay on. `guardian stop` requests release of active Guardian leases in the current Windows user session; Ctrl+C is also available in the owning terminal. User-initiated sleep, shutdown, restart, critical battery policy, Windows policy, and device behavior remain in control.
- **Network Guardian:** Experimental local observation reports Wi-Fi adapter state, gateway presence, and a tunnel-type interface hint. `guardian monitor` listens for local IP-interface and route changes with a bounded fallback snapshot. By default it sends no probes, logs no events, and performs no recovery. With explicit `--auto-reconnect-saved-wifi`, it waits 10 seconds for confirmed Wi-Fi disconnection and no default route, then makes at most three saved-profile attempts with backoff. It never restarts adapters or exposes profile names or credentials. `guardian network --probe` adds two fixed DNS lookups, active default-route inspection, bounded gateway ICMP checks, two one-shot public connectivity checks, and a preliminary core classification. Wi-Fi association does not prove Internet access. Missing probe evidence prevents a `HEALTHY` result; an absent ICMP reply remains unknown because networks often filter ping. It does not confirm VPN state. A previous candidate failed one live recovery test; the classifier correction is simulated but still needs hardware retesting.
- **Agent integration:** The Agent Skill passes metadata validation and is discoverable on the public repository. The Skills CLI copied it into a disposable Codex project, listed it, removed it, and then returned an empty project skill list. End-to-end runtime behavior remains unverified.
- **Incident reports:** Power lease events and the outcome of an explicitly requested saved-profile reconnect are stored as typed, bounded local JSONL events. Reconnect records contain an event UUID, timestamp, and redacted result category; monitor and probe observations are not journaled. The CLI can show events or export a session report. Interrupted sessions remain marked incomplete.

## Quick start

Install the Agent Skill with the standard Skills CLI:

    npx skills add abdulrahmansad/agent-guardian

This installs only the Agent Skill. If the CLI asks, select Codex. You can select it without prompts with:

    npx skills add abdulrahmansad/agent-guardian --skill agent-guardian --agent codex --yes

To remove it from a project:

    npx skills remove agent-guardian --yes

It installs only the Agent Skill; it does not install the Windows runtime or grant administrator access. No official runtime release is available. Successful PR and manual CI runs provide a temporary local MSVC candidate package for review; see the [release preparation notes](docs/release/README.md). Remote Codex project installation and removal were verified in a disposable project.

To build and run the development CLI from a Rust environment:

    cargo run -p guardian-cli -- doctor
    cargo run -p guardian-cli -- start --background --ttl-minutes 60
    cargo run -p guardian-cli -- start --background --auto-reconnect-saved-wifi --ttl-minutes 60
    cargo run -p guardian-cli -- stop

`guardian start` remains available as a foreground lease. Add `--background` to run independently of the invoking terminal; it is still a normal per-user process, not a service. Add `--auto-reconnect-saved-wifi` only when you explicitly want saved-profile reconnect attempts. `guardian stop` signals active Guardian work in the current user session. Leases expire at the configured duration and target-bound leases stop when their target exits. Start on AC power for an unattended session. On battery, the command refuses by default; `--allow-battery` caps a lease at five minutes.

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
| OpenAI Codex | Experimental. Public discovery and project installation/removal were verified in a disposable project; end-to-end runtime workflows remain unverified. |
| Other coding agents | Planned |

## Installation and permissions

The Agent Skill and the Windows runtime are separate installations. Skills CLI installation copies skill guidance into the selected agent's project or user skill directory. Run `guardian install` to install the current executable into `%LOCALAPPDATA%\Programs\AgentGuardian\guardian.exe`; it hashes the source and verifies the copied bytes, but this self-check does not verify publisher identity. Use `guardian install --sha256 DIGEST` to also check a digest obtained from a trusted release source. Installation requires no elevation and changes no PATH, startup settings, power plans, services, configuration, or logs. Close running Guardian instances before upgrading.

No runtime release has been published yet. The current local GNU/LLVM build depends on `libunwind.dll`; the installer copies only `guardian.exe`, so do not install that development build as a standalone runtime. Use `cargo run` for local development. A Windows x64 MSVC build can be installed after its runtime dependencies have been checked on a clean Windows environment and its digest computed with PowerShell:

    $exe = (Resolve-Path '.\target\x86_64-pc-windows-msvc\release\guardian.exe').Path
    $sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    & $exe install --sha256 $sha256

This local hash confirms the bytes copied into the install directory; it does not verify who built the executable. For a future release, use the checksum distributed with the release and verify that release through a trusted source. See the [runtime installation guide](docs/development/installation.md).

No service, SYSTEM process, named pipe, autostart entry, machine-wide power-plan change, firewall change, DNS change, or adapter restart is installed by the current build. Background work is limited to a same-user, same-session detached process. The protected local stop event validates a reused event's owner and access rules and lets `guardian stop` request shutdown of Guardian power and monitor processes.

## Usage

The doctor command shows available sleep states, power source, and battery percentage where Windows reports it. It reports whether the `powercfg /requests` query succeeded, but discards requestor names, process IDs, and paths. The status command reports whether that query was available and keeps requestor details redacted. If Windows denies the query, status reports it as unavailable; JSON output includes `power_request_query_available: false`. An elevated shell may be required. The network command reports local Wi-Fi adapter state, gateway presence on any adapter and on operational adapters, operational adapter counts, and whether Windows reports a tunnel-type interface. The tunnel flag is only a hint and does not confirm that a VPN is active. The command does not expose adapter names or addresses. Use `guardian --json network` for machine-readable output. Run `guardian monitor` to observe local state for up to 24 hours (default 60 minutes), with a five-minute fallback snapshot; it sends no probes. Add `--auto-reconnect-saved-wifi` to enable conservative, logged reconnect attempts. `guardian --json monitor` emits newline-delimited JSON snapshots and recovery results. `guardian start --background` runs the bounded power lease independently of the terminal; add the same opt-in recovery flag to start the monitor for that lease. `guardian stop` requests shutdown, and Ctrl+C stops foreground work.

By default, the network command reports local Wi-Fi adapter state, gateway presence, and a tunnel-type hint only. `guardian monitor` observes local IP-interface and route-table changes using Windows notifications, with a periodic local snapshot as fallback. It runs for up to 24 hours (60 minutes by default); `--interval-seconds` accepts 30 through 3600 (default 300). With `--auto-reconnect-saved-wifi`, only one monitor per Windows user session may run; it checks local state every five seconds, waits 10 seconds of confirmed Wi-Fi disconnection with no default route, then permits at most three saved-profile attempts with exponential backoff. Temporary local snapshot errors keep monitoring alive with a separate bounded retry schedule, and stale observations do not trigger recovery. It does not run external probes, restart adapters, or change settings. Events are logged without profile names, credentials, adapter identities, or addresses. The read-only mode performs no recovery or event logging. `guardian network --probe` additionally performs system DNS lookups for the two fixed endpoint names, reads active default-route metadata, sends at most one 750 ms ICMP echo to an eligible gateway for each address family, and checks two fixed web endpoints: Microsoft’s NCSI HTTP endpoint, which must return the expected `Microsoft Connect Test` text, and a Cloudflare HTTPS endpoint, which must return HTTP 200. Windows automatic proxy settings are used for the web checks, while server and proxy authentication, redirects, and cookies are disabled; a visible `407 Proxy Authentication Required` response is classified as `PROXY_RESTRICTED`. Other proxy setup or transport failures remain ambiguous. Guardian displays only boolean summaries. Gateway addresses are used in memory for route checks and ICMP and are not displayed, logged, or persisted. DNS results, route metadata, gateway response, and endpoint outcomes feed a preliminary deterministic assessment. If ICMP is filtered or any required signal is unknown, Guardian will not claim `HEALTHY`. Response text is not logged or persisted. WinHTTP receives 2-second per-stage timeout settings; DNS has a 2-second timeout and each gateway echo has a 750 ms timeout. Windows TCP retries or proxy discovery can extend total time. A one-shot probe does not start the monitor or authorize a recovery action. Adapter restart is not implemented. See [network probe details](docs/development/network-probes.md) and [monitor details](docs/development/network-monitor.md).

For one explicitly authorized Wi-Fi reconnect attempt, run `guardian network --reconnect-saved-wifi`. This can change the active Wi-Fi connection; it is refused unless Wi-Fi disconnection, no active default route, and one disconnected wireless interface are confirmed. Guardian uses one existing profile, omits its name from output, and waits up to 20 seconds for Wi-Fi association. This does not prove Internet access. Only one automatic recovery monitor may run per Windows user session. Temporary local snapshot errors retry with a bounded schedule, and stale observations cannot trigger recovery. The monitor can invoke the same bounded recovery policy only when `--auto-reconnect-saved-wifi` is explicitly enabled; it requires 10 seconds of confirmed Wi-Fi disconnection and no default route. One live test of the previous candidate produced no Guardian reconnect attempt; the classifier fix is not hardware-verified. See [saved Wi-Fi recovery details](docs/development/network-recovery.md).

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

The [configuration example](config.example.toml) documents validated defaults. `guardian start`, `guardian logs`, and `guardian report` read `%LOCALAPPDATA%\AgentGuardian\config.toml` when present, or accept `--config PATH`. `guardian network --reconnect-saved-wifi` also accepts `--config PATH` to select the local journal's size and retention limits. The CLI does not create a config file. Configuration is limited to 64 KiB; network-monitoring settings are reserved and do not independently enable monitoring or recovery in this build.

## Privacy and security

Guardian has no account requirement or mandatory telemetry. The default CLI does not send network probes. When `guardian network --probe` is used, it makes two fixed web requests through Windows automatic proxy settings, resolves their fixed hostnames through the Windows DNS namespace, reads the local route table, and may send a bounded ICMP echo to a selected default gateway. The Microsoft NCSI check uses plain HTTP; at most 64 response bytes are compared in memory and discarded. The Cloudflare check uses HTTPS and reads only the status code. Gateway IP addresses are used transiently for route and ICMP checks, then discarded; Guardian does not display or persist them. Probe results and response bodies are not written to the journal. Power-request diagnostics discard Windows command output because it can contain requestor names, process IDs, and paths. Guardian does not persist Wi-Fi passwords, SSIDs, adapter names, IP addresses, API keys, tokens, or traffic. Power lease events and explicitly requested reconnect outcomes are kept in separate, bounded local JSONL files under `%LOCALAPPDATA%\AgentGuardian`; reconnect records contain an event UUID, timestamp, and typed result category.

The initial design avoids privilege elevation and arbitrary shell commands. The installer relies on a user-supplied SHA-256 and does not authenticate publisher identity. See the [threat model](docs/security/threat-model.md) before relying on network recovery. Report vulnerabilities through a private GitHub Security Advisory when the repository enables that feature.

## Limitations

- A power request asks Windows to prevent idle sleep. Windows policy, user actions, critical battery handling, hardware, or sleep model can end or ignore it.
- Modern Standby on battery limits ordinary power requests. The development CLI caps battery leases at five minutes.
- Power source changes are checked every ten seconds while the foreground lease is active; an unsupported battery transition can therefore take up to one check interval to release.
- Win+L and screen-off behavior have not been validated on physical hardware.
- The monitor observes local interface and route metadata only. It does not test Internet, DNS, VPN, or remote-service health. Optional saved-profile reconnect requires explicit opt-in, sustained local Wi-Fi disconnection, and no default route. A test of the previous candidate failed to initiate recovery; the current fix has not been validated on hardware. Adapter restarts are not implemented.
- The `--probe` option separately performs DNS and ICMP checks plus two fixed web checks. ICMP filtering can leave gateway reachability unknown.
- Guardian cannot repair router, ISP, VPN-provider, DNS-provider, GitHub, OpenAI, npm, or other remote-service outages.
- Guardian cannot restore a terminated Codex model stream or safely replay non-idempotent operations.

## FAQ

**Does installing the skill install a Windows service?** No. The skill is guidance for a coding agent. Runtime installation is separate; the runtime installer copies one executable into the current user's profile and installs no service.

**Will the screen stay on?** Guardian requests system wakefulness only. It does not request the display to stay on.

**Can I close the lid or press Sleep and expect Guardian to override that?** No. Explicit user sleep actions and lid policy remain under Windows control.

**Will Guardian restart Wi-Fi after a failed OpenAI or GitHub endpoint?** No. Automatic reconnect only runs after local Wi-Fi disconnection with no default route; adapter restart is not implemented.

**Can Codex keep generating while offline?** Guardian cannot keep a disconnected model stream alive. An agent can continue local work that does not require the remote model or service.

## Uninstallation

Run `guardian stop` first, then `guardian uninstall` from a different Guardian executable, such as a local development build or downloaded release file, after closing other Guardian instances. The command removes only `%LOCALAPPDATA%\Programs\AgentGuardian\guardian.exe` and removes that directory only if empty. It refuses to delete the executable that is currently running. Configuration, logs, and reports under `%LOCALAPPDATA%\AgentGuardian` are preserved. Remove that directory separately only if you also want to delete Guardian's local history. Remove the Agent Skill separately using the Skills CLI's removal command for the selected agent.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md), the [architecture decisions](docs/architecture/overview.md), and [acceptance matrix](docs/testing/acceptance.md). Network-disruptive testing must use an isolated test device and requires explicit approval.

## Development

Install Rust 1.89 or newer with Cargo, rustfmt, and Clippy. The current workspace uses Rust edition 2024.

    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace --all-targets
    python scripts/check-doc-links.py
    cargo run -p guardian-cli -- doctor

Windows power and hardware behavior require Windows validation. Idle sleep, display-off, and locked-session scenarios remain untested. One live Wi-Fi recovery attempt failed for Guardian on the previous candidate; the current classifier fix needs a live retest. See the [acceptance matrix](docs/testing/acceptance.md). Use `cargo run -p guardian-cli -- --help` to inspect the current CLI.

## License

MIT. See [LICENSE](LICENSE).
