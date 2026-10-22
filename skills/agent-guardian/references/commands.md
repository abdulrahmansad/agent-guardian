# Command reference

This reference follows the current development build. Commands can change before a public release.

## Runtime availability

The skill can be installed independently with the Skills CLI. No public runtime artifact is currently distributed. Do not imply that the Skills CLI installs guardian.

## Available development commands

- guardian doctor: read-only Windows power, battery, and sleep-state diagnostics.
- guardian status: report whether Windows `powercfg /requests` can be queried while redacting requestor details. If unavailable, human output says so and `--json` sets `power_request_query_available` to `false`; an elevated shell may be required.
- guardian network: report local Wi-Fi adapter state, gateway presence, and a tunnel-type hint without network probes or changes.
- guardian monitor: listener for local IP-interface and route changes; exits after at most 24 hours (60 minutes by default), refreshes aggregate snapshots on notifications and every five minutes by default, and emits newline-delimited JSON with `--json`. It sends no probes. `--auto-reconnect-saved-wifi` explicitly enables a 10-second disconnect grace period, no-route and single-interface policy checks, at most three saved-profile attempts with backoff, and redacted outcome logging. Adapter restarts are never performed.
- guardian network --probe: explicitly run bounded DNS lookups, active default-route inspection, gateway ICMP checks, and two fixed one-shot web checks through Windows automatic proxy settings. The command shows a conservative classification; results do not authorize recovery.
- guardian network --reconnect-saved-wifi: explicitly request one reconnect using the first saved Wi-Fi profile in Windows preference order. Guardian requires confirmed Wi-Fi disconnection, no active default route, and exactly one disconnected wireless interface; it uses a 20-second Wi-Fi association confirmation window, while synchronous Windows queries can extend total command duration. It does not confirm Internet access. It does not display or save the profile name or credentials, and does not restart an adapter. The redacted result is stored in a separate bounded journal. Pass `--config PATH` to select its journal limits. This cannot be combined with `--probe`.
- guardian logs: show typed power lease events and explicit reconnect outcomes from separate local journals; `--json` emits structured events. Pass `--config PATH` to select a custom config.
- guardian report: summarize the latest power session and include reconnect outcomes whose timestamps fall inside that session's interval. `--output PATH` writes JSON without overwriting an existing path. Pass `--config PATH` to select a custom config.
- guardian start: hold a foreground lease on Guardian's own process for the configured default duration (60 minutes by default).
- guardian start --background: run the bounded lease in a detached same-user process that survives the invoking terminal.
- guardian start --background --auto-reconnect-saved-wifi: also start a monitor bound to the lease process; auto reconnect is explicitly enabled and ends with that lease.
- guardian start --pid PID --ttl-minutes MINUTES: bind a lease to a target process owned by the same Windows user and running in Guardian's session. Other users' processes and processes in a different session are refused. The configured maximum remains enforced.
- guardian start --allow-battery: permit a battery or unknown-power-source lease, capped at five minutes.
- guardian stop: send a same-user, same-session request to stop Guardian power leases and monitor processes. The command confirms signal delivery, not completed shutdown; Ctrl+C stops foreground Guardian work.
- Guardian reads `%LOCALAPPDATA%\AgentGuardian\config.toml` when present. `--config PATH` overrides that location; it never creates a config file.
- guardian version: print the runtime version.
- guardian install: hash this executable and verify the copied bytes, then install or upgrade it under `%LOCALAPPDATA%\Programs\AgentGuardian`. This self-check does not verify publisher identity.
- guardian install --sha256 DIGEST: additionally compare the executable with a digest obtained through a trusted channel. Installation makes no PATH, startup, service, configuration, or log changes and does not request elevation.
- guardian uninstall: remove the installed executable while preserving config and logs. Run it from a different Guardian executable after closing other instances; it refuses to delete its own running image.

The power lease exits when its target process exits, the same-user stop signal arrives, or the lease expires. The background worker survives the invoking terminal but is not a service. Battery behavior and locked-session operation still require hardware verification. See the repository configuration guide for the 64 KiB file limit and battery opt-in behavior.

Automatic reconnect, when explicitly enabled, acts only on confirmed local Wi-Fi disconnection with no active route. It does not diagnose upstream outages, restart adapters, or prove Internet restoration. In a 2026-10-19 live test of the previous candidate, Guardian recorded no reconnect attempt; the independent fallback's saved-profile request was accepted and Wi-Fi, route, DNS, and TCP were restored. After the classifier fix, two controlled live retests on 2026-10-22 recorded Guardian reconnect outcomes, confirmed association, and restored the default route before fallback. These are two runs on one Windows 11 x64 device/network; broader hardware coverage is not verified, and a concurrent natural Windows reconnect cannot be excluded. No official runtime checksum or release has been published. The one-shot `--probe` can suspect a captive portal from a redirect or unexpected NCSI response but does not identify VPN state or configured remote-service failures. ICMP filtering leaves gateway reachability unknown and prevents a `HEALTHY` classification. `guardian doctor` remains usable without elevation and includes a warning if Windows denies one diagnostic. Session events remain local and bounded; an interrupted session is reported without a fabricated protected duration. See the [runtime installation guide](../../../docs/development/installation.md) and [saved Wi-Fi recovery details](../../../docs/development/network-recovery.md).
