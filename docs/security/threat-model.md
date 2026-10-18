# Initial threat model

Last reviewed: 2026-10-18. This is an initial design document, not a completed security audit.

## Assets

- Availability of the user's active work session and network.
- Integrity of power and network state; normal Windows behavior must return when a lease ends.
- Privacy of local process metadata, Wi-Fi profile names, configuration, logs, and reports.
- Integrity and provenance of Guardian binaries, skill files, and future updates.

## Actors and trust boundaries

1. **User / Windows policy:** The user may stop or sleep the machine at any time; critical battery and explicit sleep behavior remain authoritative.
2. **Coding agent and skill text:** Agent output and arguments are untrusted inputs. The skill is advisory and does not grant authorization.
3. **CLI to runtime:** A caller can pass a stale/recycled PID, malformed arguments, an excessive TTL, or request repeated work. The CLI must validate values and the runtime must independently enforce bounds.
4. **Runtime to Windows APIs:** Narrow user-mode power calls are the initial OS boundary. Any future privileged helper/service requires a new design and review.
5. **Local files:** Configuration, state, logs, temporary files, and reports may be tampered with or contain attacker-controlled strings. Keep formats bounded, avoid secrets, redact identifiers, and defend against reparse-point/path surprises before persistent writes.
6. **Release/install source:** Repository files, archives, checksums, and update metadata are a software supply-chain boundary.
7. **Network:** Probes can be intercepted by VPNs, proxies, captive portals, DNS filtering, or remote outages; evidence is uncertain and must not be treated as authorization for destructive repair.

## Main abuse cases and controls

| Abuse case | Impact | Initial control / release gate |
|---|---|---|
| PID reuse or spoofed PID | A lease protects the wrong lifetime or lasts indefinitely | Open process handle, capture creation time and owner/session, verify signaling, enforce a bounded TTL; no PID-only lease |
| Stale lease after crash/restart | Unwanted permanent wake protection | Volatile leases initially; scoped request handle closes on process exit; never restore leases blindly |
| Agent requests excessive wake time | Battery drain / denial of sleep | Validate TTL and count in runtime; default AC-only or explicit battery policy; clear on expiry/exit |
| Recovery loop on GitHub/OpenAI/ISP outage | User loses healthy local Wi-Fi | Independent evidence, conservative UNKNOWN state, pure planner, strict rate limit; no adapter restart on remote-only failures |
| Malicious profile/interface string | Command injection or wrong adapter mutation | Avoid shell-based mutations; use typed native APIs and validated identities if recovery is ever enabled |
| Malicious IPC client | Unauthorized lease/recovery or privilege escalation | No IPC/service in first slice; future named pipe requires explicit DACL, local-only semantics, caller token validation, bounded schema, server-side quotas |
| Log injection or secret leakage | Misleading reports / disclosure | Power logs contain typed lease events; the separate network log contains an event UUID, timestamp, and typed reconnect result. Neither accepts arbitrary summary text, process IDs, network names, addresses, or credentials; both are size/retention bounded |
| Malicious config / path / reparse point | Unsafe limits or arbitrary file access | Configuration input is read-only, limited to 64 KiB, rejects unknown fields, and validates ranges. The event store rejects symlinked log files and a symlinked leaf directory. Ancestor reparse-point/race handling remains for dedicated review |
| Compromised artifact or update | Arbitrary code execution | No auto-update or remote download path. The per-user installer hashes the running executable against a caller-supplied digest, stages and rechecks the copy, and replaces only the fixed Guardian executable. The digest does not authenticate its publisher; the user must obtain it from a trusted release channel. No public artifact/checksum is available yet |
| Reparse-point or path substitution during install | Writes outside the intended per-user install directory | Fixed destination names under `%LOCALAPPDATA%\Programs\AgentGuardian`; source, destination, and install directories are checked for reparse points, staging uses create-new in the destination directory, and the closed staged file is rechecked and rehashed before replacement. Uninstall also compares Windows file identity to refuse deleting a hard-link alias of the running executable. Same-user concurrent filesystem tampering and ancestor-path races remain a security-review item; the installer does not elevate |

## Initial security posture

- No service, SYSTEM process, elevation, named pipe, auto-start, remote control, or arbitrary shell execution in the first slice.
- No power-plan changes, request overrides, security-setting changes, or input simulation.
- No plaintext Wi-Fi credentials or agent/API secrets in configuration or logs. Reconnect outcome logging is limited to an event UUID, timestamp, and typed result; profile and SSID data are omitted.
- The default network command sends no probes. `guardian monitor` is a bounded, foreground observer of local IP-interface and route changes. It reports aggregate adapter flags/counts and route presence only, does not send probes or change network/power state, and writes no network event details to the journal. `guardian network --probe` explicitly performs fixed DNS, route, gateway-ICMP, and web checks; it uses Windows automatic proxy settings, disables redirects and authentication, discards response text, and writes no probe results to the journal. These outputs do not authorize recovery. `guardian network --reconnect-saved-wifi` is a separate explicit action path with single-attempt and local-evidence gates; the foreground monitor cannot invoke it. It uses only the preferred saved profile, keeps its name in process memory, and logs only an event UUID, timestamp, and typed result category. It never retrieves profile XML or plaintext key material. Native API behavior on a physical network remains NOT TESTED.
- Saved-profile reconnection is opt-in and requires a controlled test device/network and explicit approval. Adapter restart remains unimplemented and is not authorized by any command.
- The installer never downloads or executes an additional payload. It preserves local configuration and logs on removal. Its supplied checksum verifies copied bytes only and is not a publisher signature or provenance proof.

## Review gates

Before enabling any automatic recovery, review authorization, native API selection, profile/adapter identity, backoff, cancellation, and logs. The one-shot reconnect path has deterministic fake-backend tests, but not controlled hardware validation. Before adding service or IPC, threat-model the service ACL, pipe DACL, caller token/PID/session checks, message limits, recovery quotas, upgrades, uninstall, and all privileged operations. Test each trust boundary with deterministic negative cases. This document does not mark those future reviews complete.
