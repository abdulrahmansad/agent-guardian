---
name: agent-guardian
description: Protect long-running coding tasks and unattended development on Windows when a user is leaving their computer, wants to prevent idle sleep, protect a Windows agent session, or asks for connectivity resilience or recovery from transient network failures.
compatibility: Windows 10 or 11 for the Guardian runtime; runtime and hardware support are still under development.
license: MIT
---

# Agent Guardian

Use this skill when the user asks to leave a coding agent working unattended, avoid Windows idle sleep, or diagnose a transient connectivity failure during development.

## Runtime setup

The Agent Skill and Guardian runtime are separate. Installing this skill does not install a service or runtime. Check for the guardian executable before using runtime commands. If it is unavailable, explain that the runtime must be installed separately; do not run a remote script or request elevation to install it.

## Power protection

Only start a Guardian lease when the user asks to protect an unattended task or approves the lease. Run guardian doctor first. Prefer AC power. Use guardian start --pid <PID> --ttl-minutes <MINUTES> when the target process is known, and state the selected process and expiry. The lease must remain bounded. The display is allowed to turn off. Tell the user how to stop the foreground command with Ctrl+C.

Guardian cannot override a user-requested sleep, shutdown, restart, critical battery behavior, or Windows policy. Do not claim that Win+L behavior or a hardware sleep model is verified unless the local diagnostics and hardware test evidence say so.

## Connectivity interruptions

When a remote operation fails, `guardian network` can report local Wi-Fi adapter operational state and gateway-address presence without sending probes. It does not verify internet or DNS health by default. Only use `guardian network --probe` when the user asks for a connectivity check; it performs bounded DNS, default-route, gateway-ICMP, and two fixed web checks, then shows a conservative assessment. Gateway addresses and DNS answer addresses are not displayed, logged, or persisted. A missing ICMP response is unknown because ping may be filtered. The diagnostic does not authorize recovery or confirm VPN or configured remote-service health. Do not infer that all services are healthy from the results. Avoid repeated requests to failing endpoints, continue independent offline work, and defer safe idempotent remote work. Never blindly replay deployments, migrations, payments, or destructive operations. Do not claim Guardian can revive a terminated model stream.

## Details

See [command reference](references/commands.md) and [safety boundaries](references/safety.md) for current implementation status and limitations.
