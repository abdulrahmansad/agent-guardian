use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;
use std::sync::mpsc;
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, HANDLE};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetBestRoute2, GetIfEntry2, GetIpForwardTable2, ICMP_ECHO_REPLY,
    ICMPV6_ECHO_REPLY_LH, IP_SUCCESS, Icmp6CreateFile, Icmp6SendEcho2, IcmpCloseHandle,
    IcmpCreateFile, IcmpSendEcho2, MIB_IF_ROW2, MIB_IPFORWARD_ROW2, MIB_IPFORWARD_TABLE2,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Networking::WinSock::{
    ADDRINFOW, AF_INET, AF_INET6, AF_UNSPEC, FreeAddrInfoW, GetAddrInfoW, IN_ADDR, IN_ADDR_0,
    IN6_ADDR, IN6_ADDR_0, INADDR_ANY, SOCKADDR_IN, SOCKADDR_IN6, SOCKADDR_IN6_0, SOCKADDR_INET,
    WSACleanup, WSADATA, WSAHOST_NOT_FOUND, WSANO_DATA, WSAStartup, WSATRY_AGAIN,
};

const DNS_TIMEOUT: Duration = Duration::from_millis(2_000);
const GATEWAY_ECHO_TIMEOUT_MS: u32 = 750;
const GATEWAY_ECHO_PAYLOAD: [u8; 1] = [0x41];
const IPV4_ROUTE_TEST_ADDRESS: [u8; 4] = [1, 1, 1, 1];
const IPV6_ROUTE_TEST_ADDRESS: [u8; 16] = [
    0x26, 0x06, 0x47, 0x00, 0x47, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0x11, 0x11,
];

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub(super) struct NetworkSignals {
    pub dns_queries_attempted: bool,
    pub microsoft_dns_resolved: Option<bool>,
    pub independent_dns_resolved: Option<bool>,
    pub dns_resolved_for_both: Option<bool>,
    pub default_route_present: Option<bool>,
    pub gateway_echo_attempted: bool,
    pub gateway_reachable: Option<bool>,
}

pub(super) fn collect_network_signals() -> NetworkSignals {
    let dns = resolve_fixed_hosts();
    let (default_route_present, gateway_targets) = route_and_gateway_targets();
    let (gateway_echo_attempted, gateway_reachable) = probe_gateways(&gateway_targets);

    NetworkSignals {
        dns_queries_attempted: dns.0,
        microsoft_dns_resolved: dns.1,
        independent_dns_resolved: dns.2,
        dns_resolved_for_both: aggregate_dns_results(dns.1, dns.2),
        default_route_present,
        gateway_echo_attempted,
        gateway_reachable,
    }
}

pub(super) fn active_default_route_status() -> Option<bool> {
    route_and_gateway_targets().0
}

struct WinsockSession;

impl WinsockSession {
    fn start() -> Option<Self> {
        let mut data = WSADATA::default();
        (unsafe { WSAStartup(0x0202, &mut data) } == 0).then_some(Self)
    }
}

impl Drop for WinsockSession {
    fn drop(&mut self) {
        unsafe {
            let _ = WSACleanup();
        }
    }
}

fn resolve_fixed_hosts() -> (bool, Option<bool>, Option<bool>) {
    let microsoft_name = wide("www.msftconnecttest.com");
    let independent_name = wide("www.cloudflare.com");
    let microsoft = resolve_dns_name(&microsoft_name);
    let independent = resolve_dns_name(&independent_name);
    (true, microsoft, independent)
}

fn resolve_dns_name(name: &[u16]) -> Option<bool> {
    let name = name.to_vec();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(query_dns_name(&name));
    });
    receiver.recv_timeout(DNS_TIMEOUT).ok().flatten()
}

fn query_dns_name(name: &[u16]) -> Option<bool> {
    let _winsock = WinsockSession::start()?;
    let mut results: *mut ADDRINFOW = ptr::null_mut();
    let result = unsafe { GetAddrInfoW(name.as_ptr(), ptr::null(), ptr::null(), &mut results) };
    if result == 0 {
        if !results.is_null() {
            unsafe { FreeAddrInfoW(results) };
            Some(true)
        } else {
            None
        }
    } else if [WSAHOST_NOT_FOUND, WSANO_DATA, WSATRY_AGAIN].contains(&result) {
        Some(false)
    } else {
        None
    }
}

fn aggregate_dns_results(first: Option<bool>, second: Option<bool>) -> Option<bool> {
    match (first, second) {
        (Some(true), Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum GatewayTarget {
    Ipv4(u32),
    Ipv6 {
        source: SOCKADDR_IN6,
        destination: SOCKADDR_IN6,
    },
}

struct RouteTable(*mut MIB_IPFORWARD_TABLE2);

impl Drop for RouteTable {
    fn drop(&mut self) {
        unsafe { FreeMibTable(self.0.cast::<c_void>()) };
    }
}

fn route_and_gateway_targets() -> (Option<bool>, Vec<GatewayTarget>) {
    let mut table: *mut MIB_IPFORWARD_TABLE2 = ptr::null_mut();
    let status = unsafe { GetIpForwardTable2(AF_UNSPEC, &mut table) };
    if status == ERROR_NOT_FOUND {
        return (Some(false), Vec::new());
    }
    if status != 0 || table.is_null() {
        return (None, Vec::new());
    }
    let _table = RouteTable(table);
    let count = unsafe { (*table).NumEntries as usize };
    let first_row = unsafe { ptr::addr_of!((*table).Table).cast::<MIB_IPFORWARD_ROW2>() };
    let mut active_default_route = false;
    let mut uncertain_interface = false;
    let mut ipv4_default_route = false;
    let mut ipv6_default_route = false;

    for index in 0..count {
        let route = unsafe { first_row.add(index).read() };
        if !is_default_prefix(&route) || route.Loopback || route.ValidLifetime == 0 {
            continue;
        }

        let mut interface = MIB_IF_ROW2 {
            InterfaceIndex: route.InterfaceIndex,
            ..MIB_IF_ROW2::default()
        };
        if unsafe { GetIfEntry2(&mut interface) } != 0 {
            uncertain_interface = true;
            continue;
        }
        if interface.OperStatus != IfOperStatusUp {
            continue;
        }
        active_default_route = true;
        let family = unsafe { route.DestinationPrefix.Prefix.si_family };
        ipv4_default_route |= family == AF_INET;
        ipv6_default_route |= family == AF_INET6;
    }

    let default_route_present = if active_default_route {
        Some(true)
    } else if uncertain_interface {
        None
    } else {
        Some(false)
    };
    let mut targets = Vec::new();
    if ipv4_default_route && let Some(target) = best_route_gateway(AF_INET) {
        targets.push(target);
    }
    if ipv6_default_route && let Some(target) = best_route_gateway(AF_INET6) {
        targets.push(target);
    }
    (default_route_present, targets)
}

fn best_route_gateway(family: u16) -> Option<GatewayTarget> {
    let mut destination = SOCKADDR_INET::default();
    match family {
        AF_INET => {
            destination.Ipv4 = SOCKADDR_IN {
                sin_family: AF_INET,
                sin_addr: IN_ADDR {
                    S_un: IN_ADDR_0 {
                        S_addr: u32::from_ne_bytes(IPV4_ROUTE_TEST_ADDRESS),
                    },
                },
                ..Default::default()
            };
        }
        AF_INET6 => {
            destination.Ipv6 = SOCKADDR_IN6 {
                sin6_family: AF_INET6,
                sin6_addr: IN6_ADDR {
                    u: IN6_ADDR_0 {
                        Byte: IPV6_ROUTE_TEST_ADDRESS,
                    },
                },
                ..Default::default()
            };
        }
        _ => return None,
    }

    let mut route = MIB_IPFORWARD_ROW2::default();
    let mut source = SOCKADDR_INET::default();
    if unsafe {
        GetBestRoute2(
            ptr::null(),
            0,
            ptr::null(),
            &destination,
            0,
            &mut route,
            &mut source,
        )
    } != 0
        || !is_default_prefix(&route)
    {
        return None;
    }
    gateway_target(&route, family, &source)
}

fn gateway_target(
    route: &MIB_IPFORWARD_ROW2,
    family: u16,
    source: &SOCKADDR_INET,
) -> Option<GatewayTarget> {
    match family {
        AF_INET => {
            let next_hop = unsafe { route.NextHop.Ipv4 };
            if next_hop.sin_family != AF_INET {
                return None;
            }
            let address = unsafe { next_hop.sin_addr.S_un.S_addr };
            (address != INADDR_ANY).then_some(GatewayTarget::Ipv4(address))
        }
        AF_INET6 => {
            let mut next_hop = unsafe { route.NextHop.Ipv6 };
            let mut source = unsafe { source.Ipv6 };
            if next_hop.sin6_family != AF_INET6 {
                return None;
            }
            if (unsafe { next_hop.sin6_addr.u.Byte }) == [0; 16]
                || source.sin6_family != AF_INET6
                || (unsafe { source.sin6_addr.u.Byte }) == [0; 16]
            {
                return None;
            }
            if unsafe { next_hop.Anonymous.sin6_scope_id } == 0 {
                next_hop.Anonymous = windows_sys::Win32::Networking::WinSock::SOCKADDR_IN6_0 {
                    sin6_scope_id: route.InterfaceIndex,
                };
            }
            if unsafe { source.Anonymous.sin6_scope_id } == 0 {
                source.Anonymous = SOCKADDR_IN6_0 {
                    sin6_scope_id: route.InterfaceIndex,
                };
            }
            Some(GatewayTarget::Ipv6 {
                source,
                destination: next_hop,
            })
        }
        _ => None,
    }
}

fn is_default_prefix(route: &MIB_IPFORWARD_ROW2) -> bool {
    if route.DestinationPrefix.PrefixLength != 0 {
        return false;
    }
    let family = unsafe { route.DestinationPrefix.Prefix.si_family };
    match family {
        AF_INET => {
            let prefix = unsafe { route.DestinationPrefix.Prefix.Ipv4 };
            (unsafe { prefix.sin_addr.S_un.S_addr }) == INADDR_ANY
        }
        AF_INET6 => {
            let prefix = unsafe { route.DestinationPrefix.Prefix.Ipv6 };
            (unsafe { prefix.sin6_addr.u.Byte }) == [0; 16]
        }
        _ => false,
    }
}

struct IcmpHandle(HANDLE);

impl IcmpHandle {
    fn new(handle: HANDLE) -> Option<Self> {
        let invalid_handle = -1_isize as HANDLE;
        (!handle.is_null() && handle != invalid_handle).then_some(Self(handle))
    }
}

impl Drop for IcmpHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = IcmpCloseHandle(self.0);
        }
    }
}

fn probe_gateways(targets: &[GatewayTarget]) -> (bool, Option<bool>) {
    let mut attempted = false;
    for target in targets {
        let replied = match target {
            GatewayTarget::Ipv4(address) => probe_ipv4_gateway(*address, &mut attempted),
            GatewayTarget::Ipv6 {
                source,
                destination,
            } => probe_ipv6_gateway(source, destination, &mut attempted),
        };
        if replied {
            return (attempted, Some(true));
        }
    }
    (attempted, None)
}

fn probe_ipv4_gateway(address: u32, attempted: &mut bool) -> bool {
    let Some(handle) = IcmpHandle::new(unsafe { IcmpCreateFile() }) else {
        return false;
    };
    let reply_size = (size_of::<ICMP_ECHO_REPLY>() + GATEWAY_ECHO_PAYLOAD.len() + 8) as u32;
    let mut reply = vec![0_u8; reply_size as usize];
    *attempted = true;
    let replies = unsafe {
        IcmpSendEcho2(
            handle.0,
            ptr::null_mut(),
            None,
            ptr::null(),
            address,
            GATEWAY_ECHO_PAYLOAD.as_ptr().cast(),
            GATEWAY_ECHO_PAYLOAD.len() as u16,
            ptr::null(),
            reply.as_mut_ptr().cast(),
            reply_size,
            GATEWAY_ECHO_TIMEOUT_MS,
        )
    };
    if replies == 0 {
        return false;
    }
    let response = unsafe { reply.as_ptr().cast::<ICMP_ECHO_REPLY>().read_unaligned() };
    response.Status == IP_SUCCESS
}

fn probe_ipv6_gateway(
    source: &SOCKADDR_IN6,
    destination: &SOCKADDR_IN6,
    attempted: &mut bool,
) -> bool {
    let Some(handle) = IcmpHandle::new(unsafe { Icmp6CreateFile() }) else {
        return false;
    };
    let reply_size = (size_of::<ICMPV6_ECHO_REPLY_LH>() + GATEWAY_ECHO_PAYLOAD.len() + 8) as u32;
    let mut reply = vec![0_u8; reply_size as usize];
    *attempted = true;
    let replies = unsafe {
        Icmp6SendEcho2(
            handle.0,
            ptr::null_mut(),
            None,
            ptr::null(),
            source,
            destination,
            GATEWAY_ECHO_PAYLOAD.as_ptr().cast(),
            GATEWAY_ECHO_PAYLOAD.len() as u16,
            ptr::null(),
            reply.as_mut_ptr().cast(),
            reply_size,
            GATEWAY_ECHO_TIMEOUT_MS,
        )
    };
    if replies == 0 {
        return false;
    }
    let response = unsafe {
        reply
            .as_ptr()
            .cast::<ICMPV6_ECHO_REPLY_LH>()
            .read_unaligned()
    };
    response.Status == IP_SUCCESS
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_evidence_is_unknown_when_fixed_hosts_disagree_or_are_unavailable() {
        assert_eq!(aggregate_dns_results(Some(true), Some(true)), Some(true));
        assert_eq!(aggregate_dns_results(Some(false), Some(false)), Some(false));
        assert_eq!(aggregate_dns_results(Some(true), Some(false)), None);
        assert_eq!(aggregate_dns_results(None, Some(false)), None);
    }

    #[test]
    fn fixed_dns_names_are_distinct_and_never_return_addresses() {
        let first = wide("www.msftconnecttest.com");
        let second = wide("www.cloudflare.com");
        assert_ne!(first, second);
        let summary = NetworkSignals::default();
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("1.1.1.1"));
        assert!(!serialized.contains("2606:"));
    }

    #[test]
    fn only_zero_prefix_routes_are_treated_as_default_routes() {
        let mut route = MIB_IPFORWARD_ROW2::default();
        route.DestinationPrefix.Prefix = windows_sys::Win32::Networking::WinSock::SOCKADDR_INET {
            Ipv4: windows_sys::Win32::Networking::WinSock::SOCKADDR_IN {
                sin_family: AF_INET,
                ..Default::default()
            },
        };
        assert!(is_default_prefix(&route));

        route.DestinationPrefix.Prefix.Ipv4.sin_addr.S_un.S_addr = 0x0101_0101;
        assert!(!is_default_prefix(&route));

        route.DestinationPrefix.PrefixLength = 24;
        route.DestinationPrefix.Prefix.Ipv4.sin_addr.S_un.S_addr = INADDR_ANY;
        assert!(!is_default_prefix(&route));
    }
}
