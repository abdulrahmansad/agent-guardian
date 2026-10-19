# Acceptance and verification matrix

Status labels: **PASS**, **FAIL**, **BLOCKED**, **NOT TESTED**. A unit test does not prove real Windows power or hardware behavior.

| # | Acceptance scenario | Status | Evidence / next check |
|---:|---|---|---|
| 1 | Protected agent remains active through idle timeout | NOT TESTED | Windows hardware run with baseline and Guardian `powercfg /requests` snapshots |
| 2 | Display can turn off while system remains awake | NOT TESTED | Verify monitor timeout and `PowerRequestSystemRequired` only |
| 3 | Normal power behavior returns after release | NOT TESTED | Stop/expiry, then wait through configured sleep timeout |
| 4 | Multiple concurrent sessions work | PASS | Deterministic `LeaseBook` test; multiple live Windows power-request handles are not tested |
| 5 | Stale/recycled PIDs do not hold protection | PASS | Deterministic process creation identity test; Windows PID reuse is not forced on this device |
| 6 | Brief Wi-Fi disconnect reconnects safely where supported | NOT TESTED | Deterministic mock-backend tests cover authorization, route/interface/profile gates, one request, and bounded confirmation; a controlled test adapter is still required |
| 7 | Naturally restored Wi-Fi does not trigger unnecessary reset | NOT TESTED | Recovery planner simulation and controlled observation |
| 8 | Upstream outage does not trigger destructive recovery | PASS | Core simulation classifies independent probe failures as upstream and returns `NotifyUser`; no recovery executor exists |
| 9 | DNS-only failure classified conservatively | PASS | Core DNS-only classifier test returns `DnsFailure` and `NotifyUser` |
| 10 | Remote GitHub failure does not reset adapter | PASS | Core configured-service simulation returns `RemoteServiceFailure` and `NotifyUser` |
| 11 | Remote OpenAI failure does not reset adapter | PASS | Core configured-service simulation returns `RemoteServiceFailure` and `NotifyUser` |
| 12 | VPN failure does not reset physical networking | PASS | Core VPN-state simulation returns `NotifyUser`; no recovery executor exists |
| 13 | Captive portal requests user action | PASS | Core captive-portal simulation returns `NotifyUser`; no recovery executor exists |
| 14 | Guardian crash leaves no permanent system changes | NOT TESTED | On Windows 11 x64, forcibly terminated only a verified one-minute test worker and confirmed its process exited. Non-elevated `guardian doctor --json` reported `powercfg /requests` unavailable, so active-request cleanup was not independently confirmed |
| 15 | Locked-session behavior is verified | NOT TESTED | Win+L on Win11 AC and supported sleep models; unit tests are insufficient |
| 16 | Battery transitions behave safely | NOT TESTED | AC/DC transition tests; Modern Standby limitations reported |
| 17 | Install/uninstall leave no unauthorized persistent changes | NOT TESTED | Clean-user VM install/uninstall audit |
| 18 | Reboot does not restore invalid leases | NOT TESTED | Restart test; expected no persisted lease restoration initially |
| 19 | Stop signal is inaccessible from another Windows user or a pre-created broad-ACL event | NOT TESTED | Windows test rejects a pre-created event with a broad ACL; verify denial from a separate Windows account |
| 20 | Reports reflect actual events | PASS | JSONL fixture CLI smoke test reconciled start/release timestamps and duration; typed reconnect outcomes are included by timestamp interval. A live power-lease event chain remains untested |
| 21 | Default local snapshot omits identifiers and changes no settings | PASS | Windows 11 x64 CLI smoke test showed counts/booleans only; a pure core test covers adapter summary aggregation; the default command does not call the probe executor |
| 22 | Configured journal size and retention are enforced | PASS | Disposable-directory integration test verifies 1 MiB rotation and removal of a file older than the configured one-day retention |
| 23 | External connectivity probes require an explicit flag | PASS | CLI parser test verifies `guardian network` defaults to local-only and `--probe` is required; probes were not sent during tests |
| 24 | Probe behavior under real DNS, route, gateway, proxy, and captive-portal conditions | NOT TESTED | Requires a controlled network; no probe was run against the active workstation |
| 25 | Connectivity assessment requires DNS, active default route, gateway response, and two independent endpoints before reporting healthy | PASS | Core tests verify complete evidence can classify `HEALTHY`, missing DNS/route/gateway evidence stays conservative, and gateway metadata from an inactive interface does not imply an operational route; live behavior remains untested |
| 26 | Explicit proxy authentication rejection is classified without retrying or changing settings | PASS | HTTP status interpretation and core policy tests cover `407` → `PROXY_RESTRICTED` → `NotifyUser`; live proxy behavior remains untested |
| 27 | Foreground local monitor starts, expires within its configured bound, and unregisters Windows notifications | PASS | Windows 11 x64 one-minute read-only JSON run emitted initial/stopped records without probes or network changes. A 35.6-second idle resource sample observed 10.44 MiB peak working set and 0.000 CPU seconds at three-decimal precision; this single sample is not a general performance claim |
| 28 | Windows interface and route notifications refresh the privacy-limited snapshot | NOT TESTED | Requires a controlled test adapter/network and an actual interface or route change; do not disrupt the active workstation |
| 29 | Checksum-verified installer and scoped uninstaller preserve unrelated files | PASS | Windows tests used disposable paths with spaces and non-ASCII characters. They covered staged-file rehashing, checksum mismatch preserving the prior executable, upgrade, removal of only `guardian.exe`, and the same-path/hard-link identity guards. Actual removal behavior while the executable image is running, a real profile install, and a clean-user VM audit remain NOT TESTED |
| 30 | Saved-profile reconnect requires explicit authorization and local Wi-Fi-only evidence | PASS | Mock-backend tests verify no request on missing authorization, active/unknown route, healthy Wi-Fi, multiple interfaces, connected interface, missing profile, route appearance during preparation, or Wi-Fi reconnecting during preparation. The native request and real Wi-Fi association remain NOT TESTED |
| 31 | Reconnect outcomes are bounded, redacted, and visible in logs/reports | PASS | Core and Windows-target tests verify typed outcome storage, omission of profile/SSID/address data, session-interval report inclusion, and preservation of the command result when logging fails. Native Wi-Fi behavior remains NOT TESTED |
| 32 | Power-request diagnostics omit requestor names, process IDs, and paths | PASS | The non-elevated Windows `guardian status` smoke check returned only a generic error with command output suppressed; focused test checks the success message contains no requestor identifiers. Successful elevated query remains NOT TESTED |
| 33 | Local adapter summary follows Windows' linked adapter list and rejects malformed links | PASS | Windows-target unit tests cover a sparse linked list, an out-of-buffer pointer, and a cycle. Live adapter enumeration during interface changes remains NOT TESTED |
| 34 | PID-bound power lease rejects another user or session | PASS | Pure policy cases reject mismatched user/session values, and a Windows-target test verifies the current process token path. Cross-user/session execution has not been performed on a separate Windows account |
| 35 | Same-user stop signal wakes the foreground lease waiter | PASS | Windows-target tests verify the protected event signals a waiter and reject a pre-created event with a broad ACL, without acquiring a power request; full power-request release remains NOT TESTED |
| 36 | Detached bounded lease and opt-in monitor start, stop, and expire | PASS | Hosted Windows [CI run 39210933404](https://github.com/abdulrahmansad/agent-guardian/actions/runs/39210933404) verifies worker acquisition, lease-bound monitor start, stop, and natural one-minute TTL expiry. On the Windows 11 x64 development laptop, an AC one-minute lease worker also started and exited after `guardian stop`. Physical sleep/display behavior, crash cleanup, and Wi-Fi recovery remain NOT TESTED |

## Automated test plan

- Core: config validation, state transitions, local adapter summary aggregation, classification, recovery decisions, backoff/rate limits, lease expiry/identity/termination, concurrent event storage, report validation, and event-schema privacy checks.
- Windows: power API failure and handle lifecycle, process creation identity, battery status, read-only diagnostics, CLI errors.
- Packaging: skill frontmatter/name/path, standard Skills CLI discovery, install/uninstall path handling, release archive/checksum verification.
- Integration: runtime acquisition/release, crash cleanup, multiple leases, permission checks for any future IPC.
- Hardware: actual idle timeout, display-off, explicit sleep, Win+L, Modern Standby/S3, AC/DC transitions, and any isolated recovery test.

## Safe test constraints

Unit/simulated network tests can run without disrupting this workstation. Do not disable/restart the active network adapter or intentionally interrupt the user's internet connection as part of automated tests. Mark hardware scenarios NOT TESTED until the exact scenario is performed and evidence is recorded.
