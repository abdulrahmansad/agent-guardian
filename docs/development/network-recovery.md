# One-shot saved Wi-Fi recovery

`guardian network --reconnect-saved-wifi` is an explicit, one-shot request to reconnect with an existing Windows Wi-Fi profile. The command may change the active wireless connection. It is separate from `guardian network --probe` and cannot be combined with it. The foreground monitor does not call this recovery path, and adapter restart is not implemented.

## Preconditions

Guardian requests a connection only when all of these checks pass:

1. The command-line option explicitly authorizes the network action.
2. The local adapter snapshot classifies Wi-Fi as disconnected.
3. The active route query confirms there is no default route. A present or unknown route blocks recovery, preserving wired and uncertain connectivity.
4. Windows reports exactly one wireless interface, and that interface is disconnected.
5. Windows returns at least one saved profile for that interface.

Immediately before calling `WlanConnect`, Guardian checks the active route and wireless interface state again. A route or Wi-Fi state change observed during preparation blocks the request. Connectivity can still change in the short interval between this final check and the Windows request.

Guardian selects the first profile in the interface's Windows preference order. Windows returns the profile name, which may also identify the SSID; Guardian keeps it only in process memory, clears the temporary copies, and omits it from terminal output, JSON, logs, and reports. Guardian does not retrieve profile XML, plaintext key material, or the separate SSID field. The native connection request is made at most once. Guardian then uses a 20-second association-confirmation window. Synchronous Windows queries can extend total command duration beyond that window. Association does not prove that a default route or Internet access is available.

The implementation uses the Windows Native Wi-Fi API rather than a shell command. It does not modify saved profiles, DNS, proxy, firewall, adapter configuration, or drivers. It never restarts an adapter. The reconnect action is not written to the power event journal.

## Results and limitations

The JSON result includes whether a connection request was accepted, whether a connection attempt was made, an optional association confirmation, a short disposition code, an optional Windows error number, and whether a fresh local snapshot was obtained afterward. It omits wireless interface IDs and profile names. A refused action reports why the evidence was insufficient or ambiguous. A timeout reports that association was not confirmed; it does not start another attempt. If the post-action snapshot fails, the command keeps the original snapshot and the recovery result.

The policy and request sequence are covered by deterministic mock-backend tests. The Windows Native Wi-Fi calls compile in the Windows-target build, but no live reconnection has been run. Physical Wi-Fi recovery, saved-profile availability, enterprise authentication, captive portals, router outages, and behavior after a manual user disconnect are **NOT TESTED**. Treat these as release gates, and do not run a disruptive test on an active workstation without explicit approval.

## Official API references

- [WlanConnect](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlanconnect) submits a connection attempt and returns immediately; notification or follow-up state is needed to determine its result. It requires access to all-user profiles when applicable.
- [WlanGetProfileList](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlangetprofilelist) returns basic profile information in preference order and requires the caller to free its returned memory with `WlanFreeMemory`.
- [WlanGetProfile](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlangetprofile) can return plaintext key material when a specific flag is requested. Guardian does not call this API.
