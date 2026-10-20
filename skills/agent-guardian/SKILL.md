---
name: agent-guardian
description: Protect long-running coding tasks and unattended development on Windows when a user is leaving their computer, wants to prevent idle sleep, protect a Windows agent session, or asks for connectivity resilience or recovery from transient network failures.
compatibility: Windows 11 x64 development host; Windows 10 and ARM64 compatibility are not verified.
license: MIT
---

# Agent Guardian

Use this skill when the user asks to leave a coding agent working unattended, avoid Windows idle sleep, or diagnose a transient connectivity failure during development.

## Runtime setup

The Agent Skill and Guardian runtime are separate. Installing this skill does not install a runtime. Check for the `guardian` executable before using runtime commands. If it is unavailable, explain that the runtime must be installed separately; no public runtime package is currently available. Do not download or install a binary without the user's request. `guardian install` verifies that the copied bytes match the invoked executable but does not authenticate its publisher; use `guardian install --sha256 <DIGEST>` when a trusted release digest is available. Installation is per-user and does not request elevation or install a service.

## Power protection

Only start a Guardian lease when the user asks to protect an unattended task or approves the lease. Run guardian doctor first. Prefer AC power. Use `guardian start --background --ttl-minutes <MINUTES>` to keep the bounded lease running independently of the invoking terminal. Add `--pid <PID>` when the target process is known; Guardian accepts only processes owned by the same Windows user and running in Guardian's session. State the selected process and expiry. The display is allowed to turn off. Tell the user they can run `guardian stop` from the same Windows user session. The stop command confirms that a request was sent; it does not confirm lease release.

Only enable automatic saved-profile reconnect when the user explicitly approves Wi-Fi changes. Use `guardian start --background --auto-reconnect-saved-wifi` for a lease-bound monitor, or `guardian monitor --auto-reconnect-saved-wifi` for a standalone bounded monitor. Only one automatic recovery monitor runs per Windows user session. It waits for 10 seconds of confirmed Wi-Fi disconnection and no active default route, then attempts a saved-profile reconnect at most three times with backoff. Temporary local snapshot errors are redacted and retried with bounded backoff; stale observations do not trigger recovery. It does not run external probes, restart adapters, or store credentials. One live test of the previous candidate produced no Guardian reconnect attempt; an independent fallback restored Wi-Fi. The classifier fix has passed simulation and CI but still needs a live retest.

Guardian cannot override a user-requested sleep, shutdown, restart, critical battery behavior, or Windows policy. Do not claim that Win+L behavior or a hardware sleep model is verified unless the local diagnostics and hardware test evidence say so.

## Connectivity interruptions

When a remote operation fails, `guardian network` can report local Wi-Fi adapter state and gateway presence without sending probes. It does not verify Internet or DNS health by default. Use `guardian monitor` for ongoing local interface/route observation; its duration is bounded to 24 hours (60 minutes by default) and it sends no probes. Automatic reconnect is a separate explicit opt-in and only reacts to confirmed local Wi-Fi disconnection with no active route. Use `guardian network --probe` only when the user asks for a connectivity check; it performs bounded DNS, default-route, gateway-ICMP, and two fixed web checks, then shows a conservative assessment. Gateway addresses and DNS answer addresses are not displayed, logged, or persisted. A missing ICMP response is unknown because ping may be filtered. The diagnostic does not authorize recovery or confirm VPN or configured remote-service health. `guardian network --reconnect-saved-wifi` and the automatic reconnect flag can change the active Wi-Fi connection, so use them only with explicit user authorization; each action uses the saved-profile policy only after confirmed Wi-Fi disconnection, no active default route, and exactly one disconnected wireless interface. Neither establishes Internet access or restarts an adapter. Do not infer that all services are healthy from probe results. Avoid repeated requests to failing endpoints, continue independent offline work, and defer safe idempotent remote work. Never blindly replay deployments, migrations, payments, or destructive operations. Do not claim Guardian can revive a terminated model stream.

## Details

See [command reference](references/commands.md) and [safety boundaries](references/safety.md) for current implementation status and limitations.
