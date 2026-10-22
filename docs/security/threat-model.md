# Initial threat model

Last reviewed: 2026-10-19. Validation evidence updated: 2026-10-22. This is an initial design document, not a completed security audit.

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
| PID reuse, spoofed PID, or cross-session target | A lease protects the wrong lifetime or another user's session | Open a process handle, capture creation time, require the target token's user SID and session to match Guardian, verify signaling, and enforce a bounded TTL; no PID-only lease |
| Sensitive power-request diagnostics | Process names or paths leak through Windows command output | Suppress `/requests` stdout and stderr; report only query availability and a generic redacted success message |
| Stale lease after crash/restart | Unwanted permanent wake protection | Volatile leases initially; scoped request handle closes on process exit; never restore leases blindly |
| Agent requests excessive wake time | Battery drain / denial of sleep | Validate TTL and count in runtime; default AC-only or explicit battery policy; clear on expiry/exit |
| Recovery loop on GitHub/OpenAI/ISP outage | User loses healthy local Wi-Fi | Independent evidence, conservative UNKNOWN state, pure planner, strict rate limit; no adapter restart on remote-only failures |
| Malicious profile/interface string | Command injection or wrong adapter mutation | Avoid shell-based mutations; use typed native APIs and validated identities if recovery is ever enabled |
| Cross-user or cross-session stop signal, including named-object squatting | Another account can end a user's power lease or make Guardian trust a broadly accessible event | Stop uses a `Local` session event whose name contains a hashed user SID and session ID. The protected DACL grants only event-control, wait, and security-query rights to the current user's SID. Before use, Guardian checks that the event owner matches the current user and that its DACL is exactly the protected single-user ACL; it fails closed on mismatch. The event accepts no command payload and can only request lease release. A different account that pre-creates the predictable name with an ACL denying Guardian access can prevent a new lease from starting in that session, but cannot hijack an active lease. Same-user software remains within the user's trust boundary and may signal or reset it |
| Log injection or secret leakage | Misleading reports / disclosure | Power logs contain typed lease events; the separate network log contains an event UUID, timestamp, and typed reconnect result. Neither accepts arbitrary summary text, process IDs, network names, addresses, or credentials; both are size/retention bounded |
| Malicious config / path / reparse point | Unsafe limits or arbitrary file access | Configuration input is read-only, limited to 64 KiB, rejects unknown fields, and validates ranges. The event store rejects symlinked log files and a symlinked leaf directory; complete ancestor reparse-point and race defenses remain future hardening |
| Compromised artifact or update | Arbitrary code execution | No auto-update or remote download path. The per-user installer hashes the running executable against a caller-supplied digest, stages and rechecks the copy, and replaces only the fixed Guardian executable. The digest does not authenticate its publisher; the user must obtain it from a trusted release channel. No public artifact/checksum is available yet |
| Reparse-point or path substitution during install | Writes outside the intended per-user install directory | Fixed destination names under `%LOCALAPPDATA%\Programs\AgentGuardian`; source, destination, and install directories are checked for reparse points, staging uses create-new in the destination directory, and the closed staged file is rechecked and rehashed before replacement. Uninstall also compares Windows file identity to refuse deleting a hard-link alias of the running executable. A 2026-10-19 focused re-review confirmed that same-user concurrent filesystem tampering and ancestor-path races remain possible within the user's writable filesystem; no privilege escalation was found. A robust mitigation requires handle-relative file operations. The installer does not elevate |

## Initial security posture

- No service, SYSTEM process, elevation, named pipe, auto-start, remote control, or arbitrary shell execution. A local stop event is the only IPC; it carries no command data and can only request release of bounded foreground leases.
- No power-plan changes, request overrides, security-setting changes, or input simulation.
- No plaintext Wi-Fi credentials or agent/API secrets in configuration or logs. Reconnect outcome logging is limited to an event UUID, timestamp, and typed result; profile and SSID data are omitted.
- The default network command sends no probes. `guardian monitor` is a bounded observer of local IP-interface and route changes. Default mode reports aggregate adapter flags/counts and route presence only, changes no network/power state, and writes no network event details. `guardian network --probe` explicitly performs fixed DNS, route, gateway-ICMP, and web checks; it uses Windows automatic proxy settings, disables redirects and authentication, discards response text, and writes no probe results to the journal. Probe output does not authorize recovery. `guardian network --reconnect-saved-wifi` is a separate explicit action path with single-attempt and local-evidence gates. The monitor can invoke the same saved-profile path only with `--auto-reconnect-saved-wifi`, after a 10-second local-disconnect grace period, and with a three-evaluation cap plus backoff. It uses only the preferred saved profile, keeps its name in process memory, and logs only an event UUID, timestamp, and typed result category. It never retrieves profile XML or plaintext key material. Two controlled live monitor recoveries on 2026-10-22 passed; broader native API behavior across hardware and networks remains NOT TESTED.
- Saved-profile reconnection is opt-in and requires a controlled test device/network and explicit approval. Adapter restart remains unimplemented and is not authorized by any command.
- The installer never downloads or executes an additional payload. It preserves local configuration and logs on removal. Its supplied checksum verifies copied bytes only and is not a publisher signature or provenance proof.

## Review gates

The opt-in automatic path uses deterministic local evidence, a grace period, a fixed retry cap, backoff, and redacted outcome logs. It has policy tests and two controlled Windows 11 x64 live validations on one device/network; broader hardware validation is still pending. A 17-minute AC idle-window test also held a power lease without a sleep event, and the system slept normally on its 15-minute idle timer after release; display-off behavior was not independently verified. The stop event's same-user creation and signaling path has Windows tests; denial from a different user account has not been tested. Before adding a service, named pipe, or broader IPC, threat-model its ACL, caller token/PID/session checks, message limits, recovery quotas, upgrades, uninstall, and all privileged operations. Test each trust boundary with deterministic negative cases. This document does not mark those future reviews complete.
