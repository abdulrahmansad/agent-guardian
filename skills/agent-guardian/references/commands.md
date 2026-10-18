# Command reference

This reference follows the current development build. Commands can change before a public release.

## Runtime availability

The skill can be installed independently with the Skills CLI. The runtime is not distributed yet. Do not imply that the Skills CLI installs guardian.

## Available development commands

- guardian doctor: read-only Windows power, battery, and sleep-state diagnostics.
- guardian status: show the Windows powercfg /requests output; this may require an elevated shell.
- guardian network: report local Wi-Fi adapter operational state and gateway-address presence without network probes or changes; it does not test internet reachability.
- guardian logs: show the typed local power lease event journal; `--json` emits structured events. Pass `--config PATH` to select a custom config.
- guardian report: summarize the latest session; `--output PATH` writes JSON without overwriting an existing path. Pass `--config PATH` to select a custom config.
- guardian start: hold a foreground lease on Guardian's own process for the configured default duration (60 minutes by default).
- guardian start --pid PID --ttl-minutes MINUTES: bind a lease to a target process and a bounded duration. The configured maximum remains enforced.
- guardian start --allow-battery: permit a battery or unknown-power-source lease, capped at five minutes.
- Guardian reads `%LOCALAPPDATA%\AgentGuardian\config.toml` when present. `--config PATH` overrides that location; it never creates a config file.
- guardian version: print the runtime version.

The foreground command exits when the target process exits or the lease expires. Ctrl+C stops Guardian. Battery behavior and locked-session operation still require hardware verification. See the repository configuration guide for the 64 KiB file limit and the persistent battery opt-in behavior.

DNS/VPN/captive-portal/upstream observation, network recovery, persistent installer, background service, and stop command are not available in the current build. `guardian doctor` remains usable without elevation and includes a warning if Windows denies one diagnostic. Session events remain local and bounded; an interrupted session is reported without a fabricated protected duration.
