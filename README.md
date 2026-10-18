# Agent Guardian

**Bounded Windows power leases and privacy-limited connectivity diagnostics.**

Agent Guardian is an early-stage Windows project for protecting long-running local AI coding sessions from idle sleep and transient connectivity failures. It combines a portable Agent Skill, an independent Rust runtime, and a conservative Windows integration.

The repository currently contains an experimental foreground power-lease implementation, bounded local session event storage, JSON/human session reports, a read-only local network snapshot, an explicitly requested one-shot connectivity check, and deterministic policy core. Continuous monitoring, network recovery, installer, and end-to-end Codex installation have not been completed. Do not rely on this development build for unattended production work.

## The use case

You start a long-running coding task in Codex and step away. Windows may enter idle sleep before the task finishes, and Wi-Fi can disconnect temporarily. Guardian is designed to request that Windows keep the system awake while leaving the display free to turn off. Its planned network layer will distinguish local Wi-Fi faults from DNS, VPN, captive-portal, upstream, and remote-service failures before recommending recovery.

Guardian cannot keep a remote model generating after its stream has ended or restore an unavailable ISP, router, or remote service.

## Initial goals

- Keep Windows from entering idle sleep during a user-authorized, bounded coding session while allowing the display to turn off.
- Detect and classify connectivity changes using multiple local and remote signals before suggesting any recovery.
- Give Codex concise, accurate Guardian state and safe retry guidance.
- Make setup, diagnostics, reports, privacy boundaries, and removal understandable to other developers.

These are project goals. The status section below identifies what the current development build actually implements.

## Project status

This repository is under active development. The current Windows CLI can request a bounded system-required power lease, inspect Windows power diagnostics, and refuse battery operation unless explicitly enabled. The core includes validated TOML configuration types, PID plus process-creation identity leases, conservative connectivity classification, and pure recovery decisions.

The default network command reports a privacy-limited local adapter snapshot and sends no probes. `guardian network --probe` is an explicit, one-shot opt-in that checks two fixed public endpoints and shows a preliminary deterministic assessment. It cannot report `HEALTHY` because DNS resolution and gateway reachability remain unknown. Continuous monitoring, DNS diagnosis, VPN classification, recovery, and network changes are not implemented. There is no service, IPC endpoint, autostart entry, updater, or release installer.

See [development status](docs/STATUS.md), [acceptance matrix](docs/testing/acceptance.md), and [threat model](docs/security/threat-model.md).

## Features

- **Power Guardian:** Experimental foreground system-required power lease. The display is not requested to stay on. User-initiated sleep, shutdown, restart, critical battery policy, Windows policy, and device behavior remain in control.
- **Network Guardian:** Experimental local observation reports Wi-Fi adapter state, gateway presence, and a tunnel-type interface hint. `guardian network --probe` adds two explicit one-shot public connectivity checks and a preliminary core classification. DNS resolution and gateway reachability remain untested, so endpoint success alone stays `SUSPECT`. It does not confirm VPN state or perform continuous monitoring. Bounded retry policy and restart budgets are implemented in the core; recovery operations are not connected.
- **Agent integration:** The Agent Skill package passes metadata validation and local Skills CLI discovery. Remote installation and end-to-end Codex behavior have not yet been validated.
- **Incident reports:** Power lease start/release events are written to a bounded local JSONL journal. The CLI can show the events or export a JSON report. Interrupted sessions remain marked incomplete; network incidents are not collected.

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

To bind a lease to an existing process:

    cargo run -p guardian-cli -- start --pid 1234 --ttl-minutes 60

Replace 1234 with the process ID. The lease ends when that process exits or the duration expires. Confirm the selected process and power source before starting a lease.

## Supported platforms

| Platform | Status |
|---|---|
| Windows 11 x64 | Development target; source is being built on Windows 11 x64. Power behavior and locked-session behavior have not been hardware-validated. |
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

The Agent Skill and the Windows runtime are separate installations. Skills CLI installation is designed to copy skill guidance into the selected agent's project or user skill directory. Runtime setup will use a verified per-user release package. The current development build has no persistent installer and does not require elevation.

No service, SYSTEM process, machine-wide power-plan change, firewall change, DNS change, or adapter restart is installed by the current build.

## Usage

The doctor command shows available sleep states, power source, and battery percentage where Windows reports it. It also shows active power requests when Windows permits access; this device requires an elevated shell for `powercfg /requests`, and doctor reports that warning without failing the other checks. The status command shows the current output from `powercfg /requests` and may require an elevated shell. The network command reports local Wi-Fi adapter state, gateway presence on any adapter and on operational adapters, operational adapter counts, and whether Windows reports a tunnel-type interface. The tunnel flag is only a hint and does not confirm that a VPN is active. The command does not expose adapter names or addresses. Use `guardian --json network` for machine-readable output. The start command holds a foreground lease and clears its request when its target exits or its lease expires. Use Ctrl+C in the owning terminal to stop it.

By default, the network command reports local Wi-Fi adapter state, gateway presence, and a tunnel-type hint only. `guardian network --probe` opts into two fixed checks: Microsoft’s NCSI HTTP endpoint, which must return the expected `Microsoft Connect Test` text, and a Cloudflare HTTPS endpoint, which must return HTTP 200. Windows automatic proxy settings are used, while server and proxy authentication, redirects, and cookies are disabled; authenticated proxies therefore fail these checks. The fixed outcomes feed a preliminary deterministic assessment, but DNS resolution and gateway reachability remain unknown, so two successful endpoint checks still classify as `SUSPECT`. Response text is not logged or persisted. WinHTTP receives 2-second per-stage timeout settings, though Windows TCP retries or proxy discovery can extend total time. These checks do not independently test DNS resolution, gateway reachability, VPN state, or configured remote services, and they do not start a monitor or recovery action. Adapter recovery is not implemented. See [network probe details](docs/development/network-probes.md).

Use `guardian logs` to list recorded power lease events. `guardian --json logs` returns machine-readable event data. `guardian report` summarizes the latest session, and `guardian report --output report.json` writes a JSON file without overwriting an existing path. An incomplete session has no reported protected duration.

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

The [configuration example](config.example.toml) documents validated defaults. `guardian start`, `guardian logs`, and `guardian report` read `%LOCALAPPDATA%\AgentGuardian\config.toml` when present, or accept `--config PATH`. The CLI does not create the file. Configuration is limited to 64 KiB; network-monitoring settings are reserved and do not enable monitoring in this build.

## Privacy and security

Guardian has no account requirement or mandatory telemetry. The default CLI does not send network probes. When `guardian network --probe` is used, it contacts the two documented endpoints through Windows automatic proxy settings. The Microsoft NCSI check uses plain HTTP as Windows does; the response is compared in a fixed 64-byte buffer and discarded. The Cloudflare check uses HTTPS and reads only the status code. No response body or endpoint result is written to the journal. Guardian does not persist Wi-Fi passwords, SSIDs, adapter names, IP addresses, API keys, tokens, or traffic. Power session events are kept locally in `%LOCALAPPDATA%\AgentGuardian` with bounded rotation; records contain only a session UUID, timestamps, and typed lease events.

The initial design avoids privilege elevation and arbitrary shell commands. See the [threat model](docs/security/threat-model.md) before relying on future recovery features. Report vulnerabilities through a private GitHub Security Advisory when the repository enables that feature.

## Limitations

- A power request asks Windows to prevent idle sleep. Windows policy, user actions, critical battery handling, hardware, or sleep model can end or ignore it.
- Modern Standby on battery limits ordinary power requests. The development CLI caps battery leases at five minutes.
- Power source changes are checked every ten seconds while the foreground lease is active; an unsupported battery transition can therefore take up to one check interval to release.
- Win+L and screen-off behavior have not been validated on physical hardware.
- The `--probe` option performs one pair of public checks only; no background network monitor or independent DNS/gateway reachability test exists. Even if both checks succeed, the preliminary assessment stays `SUSPECT`.
- Guardian cannot repair router, ISP, VPN-provider, DNS-provider, GitHub, OpenAI, npm, or other remote-service outages.
- Guardian cannot restore a terminated Codex model stream or safely replay non-idempotent operations.
- Automatic Wi-Fi reconnection and adapter recovery are not implemented.

## FAQ

**Does installing the skill install a Windows service?** No. The skill is guidance for a coding agent. Runtime installation is separate, and the current build has no service or persistent installer.

**Will the screen stay on?** Guardian requests system wakefulness only. It does not request the display to stay on.

**Can I close the lid or press Sleep and expect Guardian to override that?** No. Explicit user sleep actions and lid policy remain under Windows control.

**Will Guardian fix a failed OpenAI or GitHub endpoint by restarting Wi-Fi?** No. The core recovery policy routes remote-only failures to user notification, and live recovery is not implemented.

**Can Codex keep generating while offline?** Guardian cannot keep a disconnected model stream alive. An agent can continue local work that does not require the remote model or service.

## Uninstallation

The current development build creates no persistent runtime installation. Stop it with Ctrl+C or let its lease expire. Windows closes the request handle when the process exits. Local event files remain under `%LOCALAPPDATA%\AgentGuardian`; remove that directory if you also want to delete Guardian's local history. Remove the Agent Skill using the Skills CLI's documented removal command for the selected agent after its install workflow has been validated.

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
