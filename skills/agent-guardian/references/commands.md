# Command reference

This reference follows the current development build. Commands can change before a public release.

## Runtime availability

The skill can be installed independently with the Skills CLI. No public runtime artifact is currently distributed. Do not imply that the Skills CLI installs guardian.

## Available development commands

- guardian doctor: read-only Windows power, battery, and sleep-state diagnostics.
- guardian status: show the Windows powercfg /requests output; this may require an elevated shell.
- guardian network: report local Wi-Fi adapter state, gateway presence, and a tunnel-type hint without network probes or changes.
- guardian monitor: foreground, read-only listener for local IP-interface and route changes; exits after at most 60 minutes (default), refreshes aggregate snapshots on notifications and every five minutes by default, and emits newline-delimited JSON with `--json`. It does not send probes, perform recovery, or log network events.
- guardian network --probe: explicitly run bounded DNS lookups, active default-route inspection, gateway ICMP checks, and two fixed one-shot web checks through Windows automatic proxy settings. The command shows a conservative classification; results do not authorize recovery.
- guardian network --reconnect-saved-wifi: explicitly request one reconnect using the first saved Wi-Fi profile in Windows preference order. Guardian requires confirmed Wi-Fi disconnection, no active default route, and exactly one disconnected wireless interface; it uses a 20-second Wi-Fi association confirmation window, while synchronous Windows queries can extend total command duration. It does not confirm Internet access. It does not display or save the profile name or credentials, and does not restart an adapter. This cannot be combined with `--probe`.
- guardian logs: show the typed local power lease event journal; `--json` emits structured events. Pass `--config PATH` to select a custom config.
- guardian report: summarize the latest session; `--output PATH` writes JSON without overwriting an existing path. Pass `--config PATH` to select a custom config.
- guardian start: hold a foreground lease on Guardian's own process for the configured default duration (60 minutes by default).
- guardian start --pid PID --ttl-minutes MINUTES: bind a lease to a target process and a bounded duration. The configured maximum remains enforced.
- guardian start --allow-battery: permit a battery or unknown-power-source lease, capped at five minutes.
- Guardian reads `%LOCALAPPDATA%\AgentGuardian\config.toml` when present. `--config PATH` overrides that location; it never creates a config file.
- guardian version: print the runtime version.
- guardian install --sha256 DIGEST: verify this executable against a trusted SHA-256, then install or upgrade it under `%LOCALAPPDATA%\Programs\AgentGuardian`. It makes no PATH, startup, service, configuration, or log changes and does not request elevation.
- guardian uninstall: remove the installed executable while preserving config and logs. Run it from a different Guardian executable after closing other instances; it refuses to delete its own running image.

The foreground command exits when the target process exits or the lease expires. Ctrl+C stops Guardian. Battery behavior and locked-session operation still require hardware verification. See the repository configuration guide for the 64 KiB file limit and the persistent battery opt-in behavior.

The foreground monitor observes local interface and route metadata only; automatic recovery, adapter restart, background service, and stop command are not available. The explicit one-shot reconnect has not been exercised on a physical network. The installer accepts a supplied digest but cannot establish the publisher or provenance of an executable; no official runtime checksum or release has been published. The one-shot `--probe` can suspect a captive portal from a redirect or unexpected NCSI response but does not identify VPN state or configured remote-service failures. ICMP filtering leaves gateway reachability unknown and prevents a `HEALTHY` classification. `guardian doctor` remains usable without elevation and includes a warning if Windows denies one diagnostic. Session events remain local and bounded; an interrupted session is reported without a fabricated protected duration. See the [runtime installation guide](../../../docs/development/installation.md) and [saved Wi-Fi recovery details](../../../docs/development/network-recovery.md).
