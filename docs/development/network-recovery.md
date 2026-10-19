# Saved Wi-Fi recovery

`guardian network --reconnect-saved-wifi` is an explicit, one-shot request to reconnect with an existing Windows Wi-Fi profile. The command may change the active wireless connection. It is separate from `guardian network --probe` and cannot be combined with it. Adapter restart is not implemented.

Guardian does not infer that a disconnect was accidental. Windows' documented connection-notification payload contains profile-name, SSID, and profile-XML fields; its reason code describes operation failures and is reported as success for connection-related notifications other than `wlan_notification_acm_connection_complete`. Guardian does not register for these notifications or inspect their payload. Automatic reconnection is disabled by default. The opt-in monitor relies only on sustained local Wi-Fi disconnection and route absence; controlled hardware behavior remains unverified.

## Automatic mode

Use `guardian monitor --auto-reconnect-saved-wifi` to enable recovery for a bounded monitor session, or add the same flag to `guardian start` to bind a detached monitor to that lease. Automatic mode checks local state every five seconds, waits for 10 seconds of confirmed Wi-Fi disconnection with no active default route, and then invokes the same saved-profile policy described below. It makes at most three consecutive reconnect evaluations with 30-second exponential backoff capped at five minutes. A confirmed association pauses retries until fresh local state arrives. It sends no external probes and never restarts an adapter. Outcomes are logged to the bounded redacted network journal.

## Preconditions

Guardian requests a connection only when all of these checks pass:

1. The command-line option explicitly authorizes the network action.
2. The local adapter snapshot classifies Wi-Fi as disconnected.
3. The active route query confirms there is no default route. A present or unknown route blocks recovery, preserving wired and uncertain connectivity.
4. Windows reports exactly one wireless interface, and that interface is disconnected.
5. Windows returns at least one saved profile for that interface.

Immediately before calling `WlanConnect`, Guardian checks the active route and wireless interface state again. A route or Wi-Fi state change observed during preparation blocks the request. Connectivity can still change in the short interval between this final check and the Windows request.

In automatic mode, Guardian reads the connected profile name while Wi-Fi is available, keeps that value only in process memory, and checks that the exact profile is still saved before reconnecting. If it cannot capture or find that profile, it refuses to guess. The explicit one-shot `guardian network --reconnect-saved-wifi` command has no earlier connection context and therefore selects the first profile in Windows preference order. Windows returns profile names, which may identify SSIDs; Guardian clears temporary copies and omits them from terminal output, JSON, logs, and reports. Guardian does not retrieve profile XML, plaintext key material, or the separate SSID field. Each native connection request is made at most once. Guardian then uses a 20-second association-confirmation window. Synchronous Windows queries can extend total command duration beyond that window. Association does not prove that a default route or Internet access is available.

The implementation uses the Windows Native Wi-Fi API rather than a shell command. It does not modify saved profiles, DNS, proxy, firewall, adapter configuration, or drivers. It never restarts an adapter. The outcome is written to a separate bounded `network-events.jsonl` file, not the power lease journal. The record contains an event UUID, timestamp, and typed result category; it omits the profile name, SSID, interface ID, IP addresses, and Windows error number.

## Results and limitations

The JSON result includes whether a connection request was accepted, whether a connection attempt was made, an optional association confirmation, a short disposition code, an optional Windows error number, whether a fresh local snapshot was obtained afterward, and whether the redacted outcome was logged. It omits wireless interface IDs and profile names. A refused action reports why the evidence was insufficient or ambiguous. A timeout reports that association was not confirmed; it does not start another attempt. If the post-action snapshot or journal write fails, the command keeps and displays the recovery result. `guardian logs` reads both bounded journals. `guardian report` includes network outcomes timestamped inside the latest power session's interval; these records are not causally linked to a lease.

The policy, automatic grace/backoff schedule, and request sequence are covered by deterministic tests. The Windows Native Wi-Fi calls compile in the Windows-target build, but no live reconnection has been run. Physical Wi-Fi recovery, saved-profile availability, enterprise authentication, captive portals, router outages, and behavior after a manual user disconnect are **NOT TESTED**. Treat these as release gates, and do not run a disruptive test on an active workstation without explicit approval.

## Official API references

- [WlanConnect](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlanconnect) submits a connection attempt and returns immediately; notification or follow-up state is needed to determine its result. It requires access to all-user profiles when applicable.
- [WlanGetProfileList](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlangetprofilelist) returns basic profile information in preference order and requires the caller to free its returned memory with `WlanFreeMemory`.
- [WlanGetProfile](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlangetprofile) can return plaintext key material when a specific flag is requested. Guardian does not call this API.
- [WLAN_CONNECTION_NOTIFICATION_DATA](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/ns-wlanapi-wlan_connection_notification_data) documents the profile name, SSID, profile XML, and reason-code fields in connection notifications.
- [WlanRegisterNotification](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlanregisternotification) registers callbacks for Native Wi-Fi notifications. Guardian does not register for these notifications.
