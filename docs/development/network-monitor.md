# Local network monitor

`guardian monitor` observes local IP-interface and IP-route changes. It is separate from `guardian network --probe` and does not run external connectivity checks. Automatic saved-profile reconnection is disabled unless `--auto-reconnect-saved-wifi` is explicitly supplied.

## Run limits

```text
guardian monitor [--duration-minutes 1..1440] [--interval-seconds 30..3600]
guardian monitor --auto-reconnect-saved-wifi [--duration-minutes 1..1440]
guardian --json monitor [--duration-minutes 1..1440] [--interval-seconds 30..3600]
```

The monitor ends after 60 minutes by default and can be bounded to 24 hours. The fallback snapshot interval defaults to 300 seconds and must be between 30 seconds and one hour. Press Ctrl+C or run `guardian stop` from the same user session to stop sooner. A monitor started by `guardian start --auto-reconnect-saved-wifi` also exits when its owning power-lease process ends. Only one automatic recovery monitor can run per Windows user session. JSON output is newline-delimited: it emits an initial snapshot, changed snapshots, redacted `snapshot_unavailable` events with retry delays, recovery results when enabled, and a final stopped record.

## Observations and boundaries

Guardian registers Windows `NotifyIpInterfaceChange` and `NotifyRouteChange2` callbacks for IPv4 and IPv6. The callbacks only send a nonblocking wake signal through a bounded queue; they do not read callback row data or perform Windows queries. The foreground thread debounces notifications briefly, then reads local adapter and default-route summaries. A periodic snapshot catches changes if a notification is missed. Notification handles are canceled on normal stop and duration expiry; Windows also removes registrations when the process exits.

Output contains aggregate adapter flags/counts and whether an active default route is present. It omits interface IDs/names, SSIDs, addresses, DNS answers, and event payloads. The monitor sends no DNS, HTTP, or ICMP probes. In default mode it changes no network or power settings and writes no events. With explicit auto-reconnect enabled, it waits for 10 seconds of confirmed Wi-Fi disconnection with no active default route, then uses the existing saved-profile policy for at most three consecutive attempts with exponential backoff. It never restarts an adapter. Attempt outcomes are written to the bounded redacted network journal.

Transient local snapshot errors do not stop the monitor. The last observation is retained for display only and never used to authorize recovery while fresh data is unavailable. Snapshot queries retry with a separate backoff starting at one second and capped at 30 seconds. When data becomes fresh again, automatic recovery must observe a new 10-second disconnection grace period.

These local observations do not prove internet or remote-service health. The monitor cannot detect an upstream-only outage, DNS-only failure, captive portal, VPN state, or endpoint outage unless that condition also changes a reported local interface or route flag. The detached mode is a same-user process, not a Windows service; it ends on the bounded lease, target Guardian process, or stop signal.

## Validation status

Unit tests cover duration/interval validation, aggregate-only output, transition-only snapshot updates, redacted snapshot failures, 1–30 second snapshot backoff, one-monitor-per-session mutex cleanup, the 10-second recovery grace period, reconnect backoff and attempt cap, and waiting for fresh state after a confirmed association. A 2026-10-19 live test of the previous candidate observed the Wi-Fi loss externally but Guardian did not attempt recovery; the independent fallback made one native saved-profile request and confirmed association. The current Wi-Fi-specific classification fix is simulated only. Live Guardian notification delivery and recovery after that fix remain **NOT TESTED**.

## Microsoft references

- [NotifyIpInterfaceChange](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-notifyipinterfacechange)
- [NotifyRouteChange2](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-notifyroutechange2)
- [CancelMibChangeNotify2](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-cancelmibchangenotify2)
- [MIB_NOTIFICATION_TYPE](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/ne-netioapi-mib_notification_type)
