---
name: agent-guardian
description: Protect long-running coding tasks and unattended development on Windows when a user is leaving their computer, wants to prevent idle sleep, protect a Windows agent session, or asks for connectivity resilience or recovery from transient network failures.
compatibility: Windows 10 or 11 for the Guardian runtime; runtime and hardware support are still under development.
license: MIT
---

# Agent Guardian

Use this skill when the user asks to leave a coding agent working unattended, avoid Windows idle sleep, or diagnose a transient connectivity failure during development.

## Runtime setup

The Agent Skill and Guardian runtime are separate. Installing this skill does not install a runtime. Check for the `guardian` executable before using runtime commands. If it is unavailable, explain that the runtime must be installed separately; no public runtime package is currently available. Do not download or install a binary without the user's request and a trusted SHA-256 checksum. The per-user `guardian install --sha256 <DIGEST>` command does not install a service or request elevation.

## Power protection

Only start a Guardian lease when the user asks to protect an unattended task or approves the lease. Run guardian doctor first. Prefer AC power. Use guardian start --pid <PID> --ttl-minutes <MINUTES> when the target process is known, and state the selected process and expiry. The lease must remain bounded. The display is allowed to turn off. Tell the user how to stop the foreground command with Ctrl+C.

Guardian cannot override a user-requested sleep, shutdown, restart, critical battery behavior, or Windows policy. Do not claim that Win+L behavior or a hardware sleep model is verified unless the local diagnostics and hardware test evidence say so.

## Connectivity interruptions

When a remote operation fails, `guardian network` can report local Wi-Fi adapter operational state and gateway-address presence without sending probes. It does not verify internet or DNS health by default. Use `guardian monitor` only when the user asks for ongoing local interface/route observation; it is foreground-only, bounded to 60 minutes, sends no probes, and cannot determine internet health. Use `guardian network --probe` only when the user asks for a connectivity check; it performs bounded DNS, default-route, gateway-ICMP, and two fixed web checks, then shows a conservative assessment. Gateway addresses and DNS answer addresses are not displayed, logged, or persisted. A missing ICMP response is unknown because ping may be filtered. The diagnostic does not authorize recovery or confirm VPN or configured remote-service health. `guardian network --reconnect-saved-wifi` can change the active Wi-Fi connection, so run it only when the user explicitly authorizes that action; it makes one saved-profile request only after confirmed Wi-Fi disconnection, no active default route, and exactly one disconnected wireless interface. It does not establish Internet access or restart an adapter. Do not infer that all services are healthy from probe results. Avoid repeated requests to failing endpoints, continue independent offline work, and defer safe idempotent remote work. Never blindly replay deployments, migrations, payments, or destructive operations. Do not claim Guardian can revive a terminated model stream.

## Details

See [command reference](references/commands.md) and [safety boundaries](references/safety.md) for current implementation status and limitations.
