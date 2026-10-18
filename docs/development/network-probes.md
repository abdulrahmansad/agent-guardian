# One-shot network probes

`guardian network` is local-only. `guardian network --probe` is an explicit opt-in that sends two fixed requests once. It does not create a background monitor, save results, or authorize any recovery action.

## Requests

1. `http://www.msftconnecttest.com/connecttest.txt` must return HTTP 200 with the expected `Microsoft Connect Test` text. This follows the Windows NCSI active web-probe convention. The request is plain HTTP so a redirect or unexpected response can indicate a captive portal; it carries no credentials, cookies, query parameters, or user data. Guardian compares at most the first 64 response bytes in memory, then discards them.
2. `https://www.cloudflare.com/cdn-cgi/trace` must return HTTP 200. This request uses TLS certificate validation. Guardian reads only the status code and does not read the response body.

The endpoints are fixed in the binary. Redirects, server authentication, proxy authentication, cookies, and keep-alive are disabled. WinHTTP automatic proxy selection uses Windows user/system settings and may consult configured proxy auto-configuration. A proxy that requires authentication will not be usable for these checks. Guardian does not store endpoint responses or results in its journal.

WinHTTP receives 2-second per-stage settings for name resolution, connection, send, and receive. This is not a strict wall-clock deadline: TCP retransmission behavior and automatic proxy discovery can extend total runtime. There are no automatic retries or recurring probes.

## Interpretation limits

- A redirect or an unexpected NCSI response marks a captive portal as suspected; it is not proof.
- A failed request can reflect DNS, proxy, VPN, firewall, endpoint, or upstream problems. Guardian does not distinguish those causes from these two checks alone.
- A successful response proves only that the particular endpoint responded. It does not guarantee that GitHub, OpenAI, or another service is available.
- Probe results are displayed only. They are not logged and never authorize Wi-Fi reconnection or adapter restart.
- The probes were not run against this workstation's live network during development. Their real network behavior remains **NOT TESTED**.

## References

- [Microsoft NCSI troubleshooting and expected probe response](https://learn.microsoft.com/en-us/troubleshoot/windows-server/networking/troubleshoot-ncsi-guidance)
- [WinHttpOpen and automatic proxy behavior](https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpopen)
- [WinHttpSetTimeouts](https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpsettimouts)
- [WinHTTP redirect policy](https://learn.microsoft.com/en-us/windows/win32/winhttp/option-flags)
- [Cloudflare `/cdn-cgi/` endpoint](https://developers.cloudflare.com/fundamentals/reference/cdn-cgi-endpoint/)
