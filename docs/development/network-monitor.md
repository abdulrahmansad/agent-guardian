# Local network monitor

`guardian monitor` is an explicit, foreground observer for local IP-interface and IP-route changes. It is separate from `guardian network --probe` and does not run external connectivity checks.

## Run limits

```text
guardian monitor [--duration-minutes 1..60] [--interval-seconds 30..3600]
guardian --json monitor [--duration-minutes 1..60] [--interval-seconds 30..3600]
```

The monitor ends after 60 minutes by default. The fallback snapshot interval defaults to 300 seconds and must be between 30 seconds and one hour. Press Ctrl+C to stop sooner. JSON output is newline-delimited: it emits an initial snapshot, changed snapshots only, and a final stopped record.

## Observations and boundaries

Guardian registers Windows `NotifyIpInterfaceChange` and `NotifyRouteChange2` callbacks for IPv4 and IPv6. The callbacks only send a nonblocking wake signal through a bounded queue; they do not read callback row data or perform Windows queries. The foreground thread debounces notifications briefly, then reads local adapter and default-route summaries. A periodic snapshot catches changes if a notification is missed. Notification handles are canceled on normal stop and duration expiry; Windows also removes registrations when the process exits.

Output contains aggregate adapter flags/counts and whether an active default route is present. It omits interface IDs/names, SSIDs, addresses, DNS answers, and event payloads. The monitor sends no DNS, HTTP, or ICMP probes; it changes no network or power settings; it performs no recovery; and it writes no network events to the journal.

These local observations do not prove internet or remote-service health. The monitor cannot detect an upstream-only outage, DNS-only failure, captive portal, VPN state, or endpoint outage unless that condition also changes a reported local interface or route flag. The event callbacks and periodic snapshot are not a background service and do not continue after the command exits.

## Validation status

Unit tests cover duration/interval validation at the CLI layer, aggregate-only output, and transition-only snapshot updates. Windows compilation checks callback signatures and notification-handle management. Real interface/route-change delivery has not been tested on a controlled Windows network and remains **NOT TESTED**. Do not induce a live adapter disruption on a primary workstation.

## Microsoft references

- [NotifyIpInterfaceChange](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-notifyipinterfacechange)
- [NotifyRouteChange2](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-notifyroutechange2)
- [CancelMibChangeNotify2](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-cancelmibchangenotify2)
- [MIB_NOTIFICATION_TYPE](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/ne-netioapi-mib_notification_type)
