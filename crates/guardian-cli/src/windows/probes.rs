use std::ffi::c_void;
use windows_sys::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTP_PORT, INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_DISABLE_AUTHENTICATION, WINHTTP_DISABLE_COOKIES, WINHTTP_DISABLE_KEEP_ALIVE,
    WINHTTP_FLAG_REFRESH, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE,
    WINHTTP_OPTION_DISABLE_PROXY_AUTH_SCHEMES, WINHTTP_OPTION_REDIRECT_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY_NEVER, WINHTTP_PROXY_DISABLE_SCHEME_BASIC,
    WINHTTP_PROXY_DISABLE_SCHEME_DIGEST, WINHTTP_PROXY_DISABLE_SCHEME_KERBEROS,
    WINHTTP_PROXY_DISABLE_SCHEME_NEGOTIATE, WINHTTP_PROXY_DISABLE_SCHEME_NTLM,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
};

const STAGE_TIMEOUT_MS: i32 = 2_000;
const MAX_RESPONSE_BODY_BYTES: u32 = 64;
const MICROSOFT_EXPECTED_BODY: &[u8] = b"Microsoft Connect Test";

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub(super) struct ProbeStatus {
    pub reachable: bool,
    pub redirect_observed: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub(super) struct ProbeSummary {
    pub requests_attempted: bool,
    pub microsoft_connect_test: ProbeStatus,
    pub independent_https: ProbeStatus,
    pub captive_portal_suspected: bool,
}

struct HttpHandle(*mut c_void);

impl HttpHandle {
    fn new(handle: *mut c_void) -> Option<Self> {
        (!handle.is_null()).then_some(Self(handle))
    }
}

impl Drop for HttpHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

struct ProbeTarget {
    host: &'static str,
    path: &'static str,
    secure: bool,
    expected_status: u32,
    expected_body: Option<&'static [u8]>,
}

#[derive(Default)]
struct ProbeOutcome {
    status: ProbeStatus,
    attempted: bool,
    portal_suspected: bool,
}

pub(super) fn run_bounded_probes() -> ProbeSummary {
    let user_agent = wide("Agent Guardian connectivity check");
    let session = unsafe {
        WinHttpOpen(
            user_agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    let Some(session) = HttpHandle::new(session) else {
        return ProbeSummary::default();
    };
    let disabled_proxy_auth_schemes = WINHTTP_PROXY_DISABLE_SCHEME_BASIC
        | WINHTTP_PROXY_DISABLE_SCHEME_DIGEST
        | WINHTTP_PROXY_DISABLE_SCHEME_KERBEROS
        | WINHTTP_PROXY_DISABLE_SCHEME_NEGOTIATE
        | WINHTTP_PROXY_DISABLE_SCHEME_NTLM;
    if unsafe {
        WinHttpSetOption(
            session.0,
            WINHTTP_OPTION_DISABLE_PROXY_AUTH_SCHEMES,
            (&disabled_proxy_auth_schemes as *const u32).cast(),
            std::mem::size_of_val(&disabled_proxy_auth_schemes) as u32,
        )
    } == 0
    {
        return ProbeSummary::default();
    }
    if unsafe {
        WinHttpSetTimeouts(
            session.0,
            STAGE_TIMEOUT_MS,
            STAGE_TIMEOUT_MS,
            STAGE_TIMEOUT_MS,
            STAGE_TIMEOUT_MS,
        )
    } == 0
    {
        return ProbeSummary::default();
    }

    let microsoft = run_probe(
        session.0,
        ProbeTarget {
            host: "www.msftconnecttest.com",
            path: "/connecttest.txt",
            secure: false,
            expected_status: 200,
            expected_body: Some(MICROSOFT_EXPECTED_BODY),
        },
    );
    let independent = run_probe(
        session.0,
        ProbeTarget {
            host: "www.cloudflare.com",
            path: "/cdn-cgi/trace",
            secure: true,
            expected_status: 200,
            expected_body: None,
        },
    );
    ProbeSummary {
        requests_attempted: microsoft.attempted || independent.attempted,
        microsoft_connect_test: microsoft.status,
        independent_https: independent.status,
        captive_portal_suspected: microsoft.portal_suspected || independent.portal_suspected,
    }
}

fn run_probe(session: *mut c_void, target: ProbeTarget) -> ProbeOutcome {
    let host = wide(target.host);
    let connection = unsafe {
        WinHttpConnect(
            session,
            host.as_ptr(),
            if target.secure {
                INTERNET_DEFAULT_HTTPS_PORT
            } else {
                INTERNET_DEFAULT_HTTP_PORT
            },
            0,
        )
    };
    let Some(connection) = HttpHandle::new(connection) else {
        return ProbeOutcome::default();
    };

    let method = wide("GET");
    let path = wide(target.path);
    let request = unsafe {
        WinHttpOpenRequest(
            connection.0,
            method.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            WINHTTP_FLAG_REFRESH
                | if target.secure {
                    WINHTTP_FLAG_SECURE
                } else {
                    0
                },
        )
    };
    let Some(request) = HttpHandle::new(request) else {
        return ProbeOutcome::default();
    };

    let redirect_policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
    let redirects_disabled = unsafe {
        WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            (&redirect_policy as *const u32).cast(),
            std::mem::size_of_val(&redirect_policy) as u32,
        )
    } != 0;
    let disabled_features =
        WINHTTP_DISABLE_AUTHENTICATION | WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_KEEP_ALIVE;
    let features_disabled = unsafe {
        WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_DISABLE_FEATURE,
            (&disabled_features as *const u32).cast(),
            std::mem::size_of_val(&disabled_features) as u32,
        )
    } != 0;
    if !redirects_disabled || !features_disabled {
        return ProbeOutcome::default();
    }

    if unsafe { WinHttpSendRequest(request.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) } == 0
    {
        return ProbeOutcome {
            status: ProbeStatus::default(),
            attempted: true,
            portal_suspected: false,
        };
    }
    if unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) } == 0 {
        return ProbeOutcome {
            status: ProbeStatus::default(),
            attempted: true,
            portal_suspected: false,
        };
    }

    let mut status_code = 0_u32;
    let mut status_size = std::mem::size_of_val(&status_code) as u32;
    let mut index = 0_u32;
    if unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            std::ptr::null(),
            (&mut status_code as *mut u32).cast(),
            &mut status_size,
            &mut index,
        )
    } == 0
    {
        return ProbeOutcome {
            status: ProbeStatus::default(),
            attempted: true,
            portal_suspected: false,
        };
    }

    let body_matches = target
        .expected_body
        .map(|expected| read_body_contains(request.0, expected));
    let (status, portal_suspected) =
        interpret_response(status_code, target.expected_status, body_matches);
    ProbeOutcome {
        status,
        attempted: true,
        portal_suspected,
    }
}

fn interpret_response(
    status_code: u32,
    expected_status: u32,
    body_matches: Option<bool>,
) -> (ProbeStatus, bool) {
    let redirect_observed = (300..400).contains(&status_code);
    let body_mismatch = body_matches == Some(false);
    let reachable = status_code == expected_status && !body_mismatch;
    let portal_suspected = redirect_observed || (status_code == expected_status && body_mismatch);
    (
        ProbeStatus {
            reachable,
            redirect_observed,
        },
        portal_suspected,
    )
}

fn read_body_contains(request: *mut c_void, expected: &[u8]) -> bool {
    let mut buffer = [0_u8; MAX_RESPONSE_BODY_BYTES as usize];
    let mut bytes_read = 0_u32;
    if unsafe {
        WinHttpReadData(
            request,
            buffer.as_mut_ptr().cast(),
            MAX_RESPONSE_BODY_BYTES,
            &mut bytes_read,
        )
    } == 0
    {
        return false;
    }
    buffer[..bytes_read as usize]
        .windows(expected.len())
        .any(|window| window == expected)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_defaults_to_no_probes_or_connectivity_claims() {
        let summary = ProbeSummary::default();
        assert!(!summary.requests_attempted);
        assert!(!summary.microsoft_connect_test.reachable);
        assert!(!summary.independent_https.reachable);
        assert!(!summary.captive_portal_suspected);
    }

    #[test]
    fn microsoft_body_matching_is_bounded_and_does_not_return_response_text() {
        let valid_prefix = [b"Microsoft Connect Test".as_slice(), b"\r\n"].concat();
        assert!(
            valid_prefix
                .windows(MICROSOFT_EXPECTED_BODY.len())
                .any(|window| window == MICROSOFT_EXPECTED_BODY)
        );
        assert!(
            !b"Sign in to this network"
                .windows(MICROSOFT_EXPECTED_BODY.len())
                .any(|window| window == MICROSOFT_EXPECTED_BODY)
        );
        assert!(MAX_RESPONSE_BODY_BYTES >= MICROSOFT_EXPECTED_BODY.len() as u32);
    }

    #[test]
    fn response_interpretation_distinguishes_reachability_redirects_and_failures() {
        let (matched, matched_portal) = interpret_response(200, 200, Some(true));
        assert!(matched.reachable);
        assert!(!matched.redirect_observed);
        assert!(!matched_portal);

        let (mismatch, mismatch_portal) = interpret_response(200, 200, Some(false));
        assert!(!mismatch.reachable);
        assert!(mismatch_portal);

        let (redirect, redirect_portal) = interpret_response(302, 200, None);
        assert!(!redirect.reachable);
        assert!(redirect.redirect_observed);
        assert!(redirect_portal);

        let (unavailable, unavailable_portal) = interpret_response(503, 200, None);
        assert!(!unavailable.reachable);
        assert!(!unavailable.redirect_observed);
        assert!(!unavailable_portal);
    }
}
