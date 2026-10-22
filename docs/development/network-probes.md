# One-shot network probes

`guardian network` is local-only. `guardian network --probe` is an explicit opt-in that performs bounded DNS, route, gateway, and web checks once. It does not create a background monitor, save results, or authorize any recovery action.

## Signals

1. Guardian asks the Windows DNS namespace to resolve `www.msftconnecttest.com` and `www.cloudflare.com`. Each lookup runs in a worker thread and Guardian waits at most 2 seconds for it. A call that exceeds that wait is treated as unknown and cannot extend the command; the dispatcher has no cancellation API. The returned address list is freed immediately; only per-name booleans and the combined result are retained for the current command. A mixed result or a resolver/API error stays unknown.
2. Guardian reads the Windows IPv4/IPv6 route table and reports whether an active interface has a default route. For each address family with an active default route and a usable next hop, it asks Windows for the selected route and sends one ICMP echo to that gateway with a 750 ms timeout. The gateway address is used only in memory and is never printed, journaled, or persisted. A missing response is unknown because firewalls and networks commonly filter ICMP.
3. Guardian requests two fixed web endpoints:

   - `http://www.msftconnecttest.com/connecttest.txt` must return HTTP 200 with the expected `Microsoft Connect Test` text. This follows the Windows NCSI active web-probe convention. The request is plain HTTP so a redirect or unexpected response can indicate a captive portal; it carries no credentials, cookies, query parameters, or user data. Guardian compares at most the first 64 response bytes in memory, then discards them.
   - `https://www.cloudflare.com/cdn-cgi/trace` must return HTTP 200. This request uses TLS certificate validation. Guardian reads only the status code and does not read the response body.

The hostnames, paths, and route-query destination addresses are fixed in the binary. Redirects, server authentication, proxy authentication, cookies, and keep-alive are disabled. WinHTTP automatic proxy selection uses Windows user/system settings and may consult configured proxy auto-configuration. A `407 Proxy Authentication Required` response is classified as `PROXY_RESTRICTED`; other proxy setup or transport failures remain ambiguous. DNS, route, ICMP, and web results are displayed as summary values only and are not stored in the journal.

WinHTTP receives 2-second per-stage settings for name resolution, connection, send, and receive. Guardian waits at most 2 seconds per DNS lookup and 750 ms per gateway echo. A DNS call that outlives the wait is treated as unknown; its worker thread cannot be canceled but does not block command completion. These settings do not create a strict wall-clock deadline: TCP retransmission behavior and automatic proxy discovery can extend total runtime. There are no automatic retries or recurring probes.

## Interpretation limits

- A redirect or an unexpected NCSI response marks a captive portal as suspected; it is not proof.
- A failed web request can reflect proxy, VPN, firewall, endpoint, or upstream problems. A DNS failure is reported only when both fixed names fail through the Windows DNS namespace; a mixed result stays unknown.
- A successful response proves only that the particular endpoint responded. It does not guarantee that GitHub, OpenAI, or another service is available.
- The deterministic core requires positive DNS, active default-route, gateway-response, and two independent endpoint evidence before it can report `HEALTHY`. An absent gateway echo response is unknown rather than proof of failure, so ICMP filtering prevents a `HEALTHY` result.
- Probe results are displayed only. They are not logged and never authorize Wi-Fi reconnection or adapter restart.
- One live probe on 2026-10-22 on Windows 11 x64 returned `HEALTHY` with both DNS names resolving, a default route and gateway echo present, both web endpoints reachable, and no captive portal or proxy restriction. The earlier synchronous DNS timeout path returned `WSAEINVAL`; it is now replaced by the bounded worker-thread lookup above. Broader DNS, route, ICMP, proxy, captive-portal, and cross-network behavior remains **NOT TESTED**.

## References

- [Microsoft NCSI troubleshooting and expected probe response](https://learn.microsoft.com/en-us/troubleshoot/windows-server/networking/troubleshoot-ncsi-guidance)
- [GetAddrInfoW and DNS namespace resolution](https://learn.microsoft.com/en-us/windows/win32/api/ws2tcpip/nf-ws2tcpip-getaddrinfow)
- [GetIpForwardTable2](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-getipforwardtable2)
- [MIB_IPFORWARD_ROW2 default-route data](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/ns-netioapi-mib_ipforward_row2)
- [IcmpSendEcho2](https://learn.microsoft.com/en-us/windows/win32/api/icmpapi/nf-icmpapi-icmpsendecho2)
- [Icmp6SendEcho2](https://learn.microsoft.com/en-us/windows/win32/api/icmpapi/nf-icmpapi-icmp6sendecho2)
- [WinHttpOpen and automatic proxy behavior](https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpopen)
- [WinHttpSetTimeouts](https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpsettimouts)
- [WinHTTP redirect policy](https://learn.microsoft.com/en-us/windows/win32/winhttp/option-flags)
- [Cloudflare `/cdn-cgi/` endpoint](https://developers.cloudflare.com/fundamentals/reference/cdn-cgi-endpoint/)
