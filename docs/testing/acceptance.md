# Acceptance and verification matrix

Status labels: **PASS**, **FAIL**, **BLOCKED**, **NOT TESTED**. A unit test does not prove real Windows power or hardware behavior.

| # | Acceptance scenario | Status | Evidence / next check |
|---:|---|---|---|
| 1 | Protected agent remains active through idle timeout | NOT TESTED | Windows hardware run with baseline and Guardian `powercfg /requests` snapshots |
| 2 | Display can turn off while system remains awake | NOT TESTED | Verify monitor timeout and `PowerRequestSystemRequired` only |
| 3 | Normal power behavior returns after release | NOT TESTED | Stop/expiry, then wait through configured sleep timeout |
| 4 | Multiple concurrent sessions work | PASS | Deterministic `LeaseBook` test; multiple live Windows power-request handles are not tested |
| 5 | Stale/recycled PIDs do not hold protection | PASS | Deterministic process creation identity test; Windows PID reuse is not forced on this device |
| 6 | Brief Wi-Fi disconnect reconnects safely where supported | NOT TESTED | Simulation first; controlled test adapter only with approval |
| 7 | Naturally restored Wi-Fi does not trigger unnecessary reset | NOT TESTED | Recovery planner simulation and controlled observation |
| 8 | Upstream outage does not trigger destructive recovery | PASS | Core simulation classifies independent probe failures as upstream and returns `NotifyUser`; no recovery executor exists |
| 9 | DNS-only failure classified conservatively | PASS | Core DNS-only classifier test returns `DnsFailure` and `NotifyUser` |
| 10 | Remote GitHub failure does not reset adapter | PASS | Core configured-service simulation returns `RemoteServiceFailure` and `NotifyUser` |
| 11 | Remote OpenAI failure does not reset adapter | PASS | Core configured-service simulation returns `RemoteServiceFailure` and `NotifyUser` |
| 12 | VPN failure does not reset physical networking | PASS | Core VPN-state simulation returns `NotifyUser`; no recovery executor exists |
| 13 | Captive portal requests user action | PASS | Core captive-portal simulation returns `NotifyUser`; no recovery executor exists |
| 14 | Guardian crash leaves no permanent system changes | NOT TESTED | Process termination test and post-crash `powercfg /requests` |
| 15 | Locked-session behavior is verified | NOT TESTED | Win+L on Win11 AC and supported sleep models; unit tests are insufficient |
| 16 | Battery transitions behave safely | NOT TESTED | AC/DC transition tests; Modern Standby limitations reported |
| 17 | Install/uninstall leave no unauthorized persistent changes | NOT TESTED | Clean-user VM install/uninstall audit |
| 18 | Reboot does not restore invalid leases | NOT TESTED | Restart test; expected no persisted lease restoration initially |
| 19 | Service permissions resist unauthorized clients | NOT TESTED | No service/IPC planned in first slice; gate before adding it |
| 20 | Reports reflect actual events | PASS | JSONL fixture CLI smoke test reconciled start/release timestamps and duration; a live power-lease event chain remains untested |
| 21 | Default local snapshot omits identifiers and changes no settings | PASS | Windows 11 x64 CLI smoke test showed counts/booleans only; a pure core test covers adapter summary aggregation; the default command does not call the probe executor |
| 22 | Configured journal size and retention are enforced | PASS | Disposable-directory integration test verifies 1 MiB rotation and removal of a file older than the configured one-day retention |
| 23 | External connectivity probes require an explicit flag | PASS | CLI parser test verifies `guardian network` defaults to local-only and `--probe` is required; probes were not sent during tests |
| 24 | Probe behavior under real DNS, route, gateway, proxy, and captive-portal conditions | NOT TESTED | Requires a controlled network; no probe was run against the active workstation |
| 25 | Connectivity assessment requires DNS, active default route, gateway response, and two independent endpoints before reporting healthy | PASS | Core tests verify complete evidence can classify `HEALTHY`, missing DNS/route/gateway evidence stays conservative, and gateway metadata from an inactive interface does not imply an operational route; live behavior remains untested |
| 26 | Explicit proxy authentication rejection is classified without retrying or changing settings | PASS | HTTP status interpretation and core policy tests cover `407` → `PROXY_RESTRICTED` → `NotifyUser`; live proxy behavior remains untested |

## Automated test plan

- Core: config validation, state transitions, local adapter summary aggregation, classification, recovery decisions, backoff/rate limits, lease expiry/identity/termination, concurrent event storage, report validation, and event-schema privacy checks.
- Windows: power API failure and handle lifecycle, process creation identity, battery status, read-only diagnostics, CLI errors.
- Packaging: skill frontmatter/name/path, standard Skills CLI discovery, install/uninstall path handling, release archive/checksum verification.
- Integration: runtime acquisition/release, crash cleanup, multiple leases, permission checks for any future IPC.
- Hardware: actual idle timeout, display-off, explicit sleep, Win+L, Modern Standby/S3, AC/DC transitions, and any isolated recovery test.

## Safe test constraints

Unit/simulated network tests can run without disrupting this workstation. Do not disable/restart the active network adapter or intentionally interrupt the user's internet connection as part of automated tests. Mark hardware scenarios NOT TESTED until the exact scenario is performed and evidence is recorded.
