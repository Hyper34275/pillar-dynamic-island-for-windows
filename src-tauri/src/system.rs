//! System identity for the About tab and diagnostics: computer, user, OS, WebView2
//! and the local IPv4 address. Win32 only, read only, no network traffic.
//!
//! The local IPv4 follows the documented strategy of `docs/ENTERPRISE_DESIGN.md` §4;
//! the scoring is the pure [`select_ipv4`]. The result is cached and invalidated by
//! `NotifyIpInterfaceChange` / `NotifyUnicastIpAddressChange`; nothing polls.

use crate::{debug_log, rt};
use serde::Serialize;
use std::ffi::c_void;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use windows::core::{HSTRING, PWSTR};
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetAdaptersAddresses, GetIpForwardTable2, NotifyIpInterfaceChange, NotifyUnicastIpAddressChange,
    GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
    IP_ADAPTER_ADDRESSES_LH, MIB_IPFORWARD_TABLE2, MIB_IPINTERFACE_ROW, MIB_NOTIFICATION_TYPE,
    MIB_UNICASTIPADDRESS_ROW,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameSamCompatible};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::SystemInformation::{ComputerNameNetBIOS, GetComputerNameExW, OSVERSIONINFOW};
use windows::Win32::System::Threading::GetCurrentProcessId;

const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_PPP: u32 = 23;
const IF_TYPE_LOOPBACK: u32 = 24;
const IF_TYPE_WIFI: u32 = 71;
const IF_TYPE_TUNNEL: u32 = 131;
const WINDOWS_11_FIRST_BUILD: u32 = 22000;
const CURRENT_VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub computer_name: String,
    pub local_ipv4: Option<String>,
    /// Adapter friendly name, for diagnostics only.
    pub ip_adapter: Option<String>,
    /// `DOMAIN\USER`
    pub windows_user: String,
    pub session_id: u32,
    pub os_name: String,
    pub os_display_version: Option<String>,
    pub os_build: u32,
    pub app_version: String,
    pub webview2_version: Option<String>,
}

// =============================================================================
// Registry (read only)
// =============================================================================

pub fn hklm_string(subkey: &str, value: &str) -> Option<String> {
    let subkey = HSTRING::from(subkey);
    let value = HSTRING::from(value);
    let mut bytes = 0u32;
    unsafe {
        let status = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &value,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        );
        if status != ERROR_SUCCESS || bytes < 2 {
            return None;
        }
        let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
        let status = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &value,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut c_void),
            Some(&mut bytes),
        );
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len])).filter(|s| !s.is_empty())
    }
}

pub fn hklm_dword(subkey: &str, value: &str) -> Option<u32> {
    let subkey = HSTRING::from(subkey);
    let value = HSTRING::from(value);
    let mut data = 0u32;
    let mut bytes = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &value,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut c_void),
            Some(&mut bytes),
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

// =============================================================================
// Identity (constant for the life of the process)
// =============================================================================

struct Identity {
    computer_name: String,
    windows_user: String,
    session_id: u32,
    os_name: &'static str,
    os_display_version: Option<String>,
    os_build: u32,
    webview2_version: Option<String>,
}

fn os_name(build: u32) -> &'static str {
    if build >= WINDOWS_11_FIRST_BUILD {
        "Windows 11"
    } else {
        "Windows 10"
    }
}

fn os_build() -> u32 {
    let mut info = OSVERSIONINFOW { dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
    // RtlGetVersion is not subject to the manifest-based version lie of GetVersionEx.
    if unsafe { RtlGetVersion(&mut info) }.is_ok() {
        info.dwBuildNumber
    } else {
        0
    }
}

fn computer_name() -> String {
    let mut buffer = [0u16; 64];
    let mut len = buffer.len() as u32;
    let name = unsafe { GetComputerNameExW(ComputerNameNetBIOS, PWSTR(buffer.as_mut_ptr()), &mut len) }
        .ok()
        .map(|_| String::from_utf16_lossy(&buffer[..len as usize]))
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_default();
    name.to_uppercase()
}

fn windows_user() -> String {
    let mut buffer = [0u16; 256];
    let mut len = buffer.len() as u32;
    let sam = unsafe { GetUserNameExW(NameSamCompatible, PWSTR(buffer.as_mut_ptr()), &mut len) };
    if sam.as_bool() && len > 0 {
        return String::from_utf16_lossy(&buffer[..len as usize]);
    }
    let user = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    }
}

fn session_id() -> u32 {
    let mut session = 0u32;
    unsafe {
        let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut session);
    }
    session
}

fn identity() -> &'static Identity {
    static IDENTITY: OnceLock<Identity> = OnceLock::new();
    IDENTITY.get_or_init(|| {
        let build = os_build();
        Identity {
            computer_name: computer_name(),
            windows_user: windows_user(),
            session_id: session_id(),
            os_name: os_name(build),
            os_display_version: hklm_string(CURRENT_VERSION_KEY, "DisplayVersion")
                .or_else(|| hklm_string(CURRENT_VERSION_KEY, "ReleaseId")),
            os_build: build,
            webview2_version: tauri::webview_version().ok().filter(|v| !v.is_empty()),
        }
    })
}

pub fn info() -> SystemInfo {
    let id = identity();
    let ip = local_ipv4();
    SystemInfo {
        computer_name: id.computer_name.clone(),
        local_ipv4: ip.as_ref().map(|s| s.ip.to_string()),
        ip_adapter: ip.map(|s| s.adapter),
        windows_user: id.windows_user.clone(),
        session_id: id.session_id,
        os_name: id.os_name.to_string(),
        os_display_version: id.os_display_version.clone(),
        os_build: id.os_build,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        webview2_version: id.webview2_version.clone(),
    }
}

#[tauri::command]
pub async fn get_system_info() -> Result<SystemInfo, String> {
    rt::run_blocking("get_system_info", || Ok(info())).await
}

// =============================================================================
// Local IPv4 selection (docs/ENTERPRISE_DESIGN.md §4)
// =============================================================================

/// One IPv4 address of one adapter, as collected from `GetAdaptersAddresses`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterCandidate {
    pub if_index: u32,
    pub name: String,
    pub description: String,
    pub if_type: u32,
    pub up: bool,
    pub ip: Ipv4Addr,
    /// Lowest route metric + interface metric of this adapter's `0.0.0.0/0` route that
    /// has a non-zero gateway; `None` when it owns no default route.
    pub default_route_metric: Option<u32>,
    pub if_metric: u32,
}

const VIRTUAL_SUBSTRINGS: [&str; 14] = [
    "vethernet", "hyper-v", "virtualbox", "vmware", "vmnet", "tailscale", "wsl", "docker", "tap-windows", "npcap",
    "loopback", "bluetooth", "virtual", "zerotier",
];
const VIRTUAL_TOKENS: [&str; 2] = ["tap", "tun"];
const VPN_SUBSTRINGS: [&str; 10] = [
    "vpn", "anyconnect", "globalprotect", "pangp", "forti", "wireguard", "wintun", "nordlynx", "sonicwall", "juniper",
];

fn haystack(a: &AdapterCandidate) -> String {
    format!("{} {}", a.name, a.description).to_lowercase()
}

fn is_virtual(a: &AdapterCandidate) -> bool {
    let text = haystack(a);
    VIRTUAL_SUBSTRINGS.iter().any(|p| text.contains(p))
        || text.split(|c: char| !c.is_alphanumeric()).any(|t| VIRTUAL_TOKENS.contains(&t))
}

fn is_vpn(a: &AdapterCandidate) -> bool {
    let text = haystack(a);
    a.if_type == IF_TYPE_PPP || VPN_SUBSTRINGS.iter().any(|p| text.contains(p))
}

/// Lower is better: physical before VPN, owner of a default route before others, lowest
/// route metric, Ethernet > Wi-Fi > other, lowest interface metric, lowest IfIndex.
fn rank(a: &AdapterCandidate) -> (bool, bool, u32, u8, u32, u32) {
    let type_rank = match a.if_type {
        IF_TYPE_ETHERNET => 0,
        IF_TYPE_WIFI => 1,
        _ => 2,
    };
    (
        is_vpn(a),
        a.default_route_metric.is_none(),
        a.default_route_metric.unwrap_or(u32::MAX),
        type_rank,
        a.if_metric,
        a.if_index,
    )
}

/// Pick the adapter whose IPv4 address is "the" LAN address, or `None` (NET-301).
///
/// Up adapters that are not loopback/tunnel and have a non-loopback address qualify.
/// Link-local (APIPA) addresses are used only when nothing else exists; virtual
/// adapters (Hyper-V, VMware, Docker, ...) only when no physical one remains.
pub fn select_ipv4(candidates: &[AdapterCandidate]) -> Option<&AdapterCandidate> {
    let usable: Vec<&AdapterCandidate> = candidates
        .iter()
        .filter(|a| {
            a.up && a.if_type != IF_TYPE_LOOPBACK && a.if_type != IF_TYPE_TUNNEL && !a.ip.is_loopback() && !a.ip.is_unspecified()
        })
        .collect();
    let routable: Vec<&AdapterCandidate> = usable.iter().copied().filter(|a| !a.ip.is_link_local()).collect();
    let pool = if routable.is_empty() { usable } else { routable };
    let physical: Vec<&AdapterCandidate> = pool.iter().copied().filter(|a| !is_virtual(a)).collect();
    let pool = if physical.is_empty() { pool } else { physical };
    pool.into_iter().min_by_key(|a| rank(a))
}

unsafe fn wide_to_string(p: PWSTR) -> String {
    if p.is_null() {
        String::new()
    } else {
        p.to_string().unwrap_or_default()
    }
}

/// `(interface index, route metric)` of every IPv4 default route with a gateway.
fn default_routes() -> Vec<(u32, u32)> {
    let mut routes = Vec::new();
    unsafe {
        let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
        if GetIpForwardTable2(AF_INET, &mut table).is_err() || table.is_null() {
            return routes;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        for row in rows {
            let gateway = row.NextHop.Ipv4.sin_addr.S_un.S_addr;
            if row.DestinationPrefix.PrefixLength == 0 && row.NextHop.si_family == AF_INET && gateway != 0 {
                routes.push((row.InterfaceIndex, row.Metric));
            }
        }
        FreeMibTable(table as *const c_void);
    }
    routes
}

fn collect_candidates() -> Result<Vec<AdapterCandidate>, String> {
    let routes = default_routes();
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER | GAA_FLAG_INCLUDE_GATEWAYS;
    // u64 storage keeps the buffer 8-byte aligned for IP_ADAPTER_ADDRESSES_LH.
    let mut buffer: Vec<u64> = vec![0; 16 * 1024 / 8];
    let mut bytes = (buffer.len() * 8) as u32;
    for _ in 0..4 {
        let status = unsafe {
            GetAdaptersAddresses(
                AF_INET.0 as u32,
                flags,
                None,
                Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut bytes,
            )
        };
        if status == ERROR_BUFFER_OVERFLOW.0 {
            buffer = vec![0; (bytes as usize).div_ceil(8)];
            continue;
        }
        if status != ERROR_SUCCESS.0 {
            return Err(format!("GetAdaptersAddresses failed ({status})"));
        }
        return Ok(parse_adapters(buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH, &routes));
    }
    Err("GetAdaptersAddresses kept overflowing".to_string())
}

fn parse_adapters(first: *const IP_ADAPTER_ADDRESSES_LH, routes: &[(u32, u32)]) -> Vec<AdapterCandidate> {
    let mut out = Vec::new();
    let mut adapter = first;
    unsafe {
        while !adapter.is_null() {
            let a = &*adapter;
            let if_index = a.Anonymous1.Anonymous.IfIndex;
            let mut unicast = a.FirstUnicastAddress;
            // Prefer a routable address over an APIPA one on the same adapter.
            let mut chosen: Option<Ipv4Addr> = None;
            while !unicast.is_null() {
                let u = &*unicast;
                let sockaddr = u.Address.lpSockaddr;
                if !sockaddr.is_null() && (*sockaddr).sa_family == AF_INET {
                    let raw = (*(sockaddr as *const SOCKADDR_IN)).sin_addr.S_un.S_addr;
                    let ip = Ipv4Addr::from(raw.to_ne_bytes());
                    if chosen.is_none() || (chosen.is_some_and(|c| c.is_link_local()) && !ip.is_link_local()) {
                        chosen = Some(ip);
                    }
                }
                unicast = u.Next;
            }
            if let Some(ip) = chosen {
                let default_route_metric = routes
                    .iter()
                    .filter(|(index, _)| *index == if_index)
                    .map(|(_, metric)| metric.saturating_add(a.Ipv4Metric))
                    .min();
                out.push(AdapterCandidate {
                    if_index,
                    name: wide_to_string(a.FriendlyName),
                    description: wide_to_string(a.Description),
                    if_type: a.IfType,
                    up: a.OperStatus == IfOperStatusUp,
                    ip,
                    default_route_metric,
                    if_metric: a.Ipv4Metric,
                });
            }
            adapter = a.Next;
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct IpSelection {
    pub ip: Ipv4Addr,
    pub adapter: String,
}

static IP_CACHE: Mutex<Option<Option<IpSelection>>> = Mutex::new(None);
/// Bumped on every network change so a selection computed from an older snapshot is
/// never cached (a notification can arrive while `GetAdaptersAddresses` is running).
static IP_GENERATION: AtomicU64 = AtomicU64::new(0);
static NO_IP_LOGGED: AtomicBool = AtomicBool::new(false);
static NOTIFY_ONCE: Once = Once::new();

fn invalidate_ip_cache() {
    let mut cache = IP_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    IP_GENERATION.fetch_add(1, Ordering::SeqCst);
    *cache = None;
}

unsafe extern "system" fn on_interface_change(_ctx: *const c_void, _row: *const MIB_IPINTERFACE_ROW, _kind: MIB_NOTIFICATION_TYPE) {
    debug_log::catch("net", invalidate_ip_cache);
}

unsafe extern "system" fn on_address_change(_ctx: *const c_void, _row: *const MIB_UNICASTIPADDRESS_ROW, _kind: MIB_NOTIFICATION_TYPE) {
    debug_log::catch("net", invalidate_ip_cache);
}

/// Register for network changes once; the registrations live for the whole process.
fn watch_network() {
    NOTIFY_ONCE.call_once(|| unsafe {
        let mut handle = HANDLE::default();
        if NotifyIpInterfaceChange(AF_INET, Some(on_interface_change), None, false, &mut handle).is_err() {
            dlog!("WARN", "net", "NET-301 interface change notifications unavailable");
        }
        let mut handle = HANDLE::default();
        if NotifyUnicastIpAddressChange(AF_INET, Some(on_address_change), None, false, &mut handle).is_err() {
            dlog!("WARN", "net", "NET-301 address change notifications unavailable");
        }
    });
}

/// The selected LAN IPv4 (cached until the network changes), `None` = NET-301.
pub fn local_ipv4() -> Option<IpSelection> {
    watch_network();
    if let Some(cached) = IP_CACHE.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return cached;
    }
    let generation = IP_GENERATION.load(Ordering::SeqCst);
    let selection = match collect_candidates() {
        Ok(candidates) => select_ipv4(&candidates).map(|a| IpSelection { ip: a.ip, adapter: a.name.clone() }),
        Err(e) => {
            dlog!("WARN", "net", "NET-301 {}", e);
            None
        }
    };
    if selection.is_none() {
        if !NO_IP_LOGGED.swap(true, Ordering::Relaxed) {
            dlog!("WARN", "net", "NET-301 no usable LAN IPv4 address");
        }
    } else {
        NO_IP_LOGGED.store(false, Ordering::Relaxed);
    }
    let mut cache = IP_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if IP_GENERATION.load(Ordering::SeqCst) == generation {
        *cache = Some(selection.clone());
    }
    selection
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(if_index: u32, name: &str, description: &str, if_type: u32, ip: [u8; 4]) -> AdapterCandidate {
        AdapterCandidate {
            if_index,
            name: name.to_string(),
            description: description.to_string(),
            if_type,
            up: true,
            ip: Ipv4Addr::from(ip),
            default_route_metric: None,
            if_metric: 25,
        }
    }

    fn ethernet(if_index: u32, ip: [u8; 4]) -> AdapterCandidate {
        adapter(if_index, "Ethernet", "Intel(R) Ethernet Connection I219-LM", IF_TYPE_ETHERNET, ip)
    }

    fn wifi(if_index: u32, ip: [u8; 4]) -> AdapterCandidate {
        adapter(if_index, "Wi-Fi", "Intel(R) Wi-Fi 6 AX201 160MHz", IF_TYPE_WIFI, ip)
    }

    fn with_route(mut a: AdapterCandidate, metric: u32) -> AdapterCandidate {
        a.default_route_metric = Some(metric);
        a
    }

    fn picked(candidates: &[AdapterCandidate]) -> Option<Ipv4Addr> {
        select_ipv4(candidates).map(|a| a.ip)
    }

    #[test]
    fn table_of_selection_cases() {
        let ip = |a: [u8; 4]| Some(Ipv4Addr::from(a));
        let vether = adapter(9, "vEthernet (Default Switch)", "Hyper-V Virtual Ethernet Adapter", IF_TYPE_ETHERNET, [172, 20, 0, 1]);
        let vpn = adapter(11, "Corp VPN", "Acme Secure Connect Adapter", IF_TYPE_ETHERNET, [10, 99, 0, 5]);
        let mut down = ethernet(2, [192, 168, 1, 50]);
        down.up = false;
        let apipa = ethernet(3, [169, 254, 7, 7]);
        let loopback = adapter(1, "Loopback Pseudo-Interface 1", "Software Loopback Interface 1", IF_TYPE_LOOPBACK, [127, 0, 0, 1]);
        let tunnel = adapter(15, "Teredo Tunneling Pseudo-Interface", "Teredo", IF_TYPE_TUNNEL, [192, 0, 2, 9]);

        struct Case {
            name: &'static str,
            candidates: Vec<AdapterCandidate>,
            expected: Option<Ipv4Addr>,
        }
        let cases = [
            Case { name: "empty", candidates: vec![], expected: None },
            Case { name: "only loopback and tunnel", candidates: vec![loopback.clone(), tunnel.clone()], expected: None },
            Case { name: "down adapter ignored", candidates: vec![down.clone()], expected: None },
            Case {
                name: "ethernet wins over wifi without routes",
                candidates: vec![wifi(5, [192, 168, 1, 20]), ethernet(6, [10, 0, 0, 7])],
                expected: ip([10, 0, 0, 7]),
            },
            Case {
                name: "route owner wins over ethernet without route",
                candidates: vec![ethernet(6, [10, 0, 0, 7]), with_route(wifi(5, [192, 168, 1, 20]), 35)],
                expected: ip([192, 168, 1, 20]),
            },
            Case {
                name: "lowest route metric wins",
                candidates: vec![with_route(ethernet(6, [10, 0, 0, 7]), 281), with_route(wifi(5, [192, 168, 1, 20]), 55)],
                expected: ip([192, 168, 1, 20]),
            },
            Case {
                name: "virtual adapter dropped when a physical one exists",
                candidates: vec![with_route(vether.clone(), 10), ethernet(6, [10, 0, 0, 7])],
                expected: ip([10, 0, 0, 7]),
            },
            Case {
                name: "virtual adapter used when nothing else remains",
                candidates: vec![vether.clone(), loopback.clone(), down.clone()],
                expected: ip([172, 20, 0, 1]),
            },
            Case {
                name: "vpn with the best route ranks below physical",
                candidates: vec![with_route(vpn.clone(), 1), with_route(wifi(5, [192, 168, 1, 20]), 55)],
                expected: ip([192, 168, 1, 20]),
            },
            Case { name: "vpn alone is acceptable", candidates: vec![vpn.clone()], expected: ip([10, 99, 0, 5]) },
            Case {
                name: "apipa only as a last resort",
                candidates: vec![apipa.clone(), vether.clone()],
                expected: ip([172, 20, 0, 1]),
            },
            Case { name: "apipa alone", candidates: vec![apipa.clone()], expected: ip([169, 254, 7, 7]) },
            Case {
                name: "real address beats apipa",
                candidates: vec![apipa.clone(), wifi(5, [192, 168, 1, 20])],
                expected: ip([192, 168, 1, 20]),
            },
            Case {
                name: "tie on type and metric: lowest if index",
                candidates: vec![ethernet(8, [10, 0, 0, 8]), ethernet(4, [10, 0, 0, 4])],
                expected: ip([10, 0, 0, 4]),
            },
            Case {
                name: "tie on type: lowest interface metric",
                candidates: {
                    let mut a = ethernet(4, [10, 0, 0, 4]);
                    a.if_metric = 40;
                    let mut b = ethernet(8, [10, 0, 0, 8]);
                    b.if_metric = 10;
                    vec![a, b]
                },
                expected: ip([10, 0, 0, 8]),
            },
            Case {
                name: "other adapter types rank after wifi",
                candidates: vec![adapter(2, "Mobile Broadband", "Sierra Wireless EM7565", 243, [100, 64, 0, 9]), wifi(5, [192, 168, 1, 20])],
                expected: ip([192, 168, 1, 20]),
            },
            Case {
                name: "full mix",
                candidates: vec![
                    loopback,
                    tunnel,
                    down,
                    apipa,
                    vether,
                    with_route(vpn, 1),
                    wifi(5, [192, 168, 1, 20]),
                    with_route(ethernet(6, [10, 0, 0, 7]), 25),
                ],
                expected: ip([10, 0, 0, 7]),
            },
        ];
        for case in cases {
            assert_eq!(picked(&case.candidates), case.expected, "{}", case.name);
        }
    }

    #[test]
    fn virtual_and_vpn_detection_avoids_false_positives() {
        let real = ethernet(1, [10, 0, 0, 1]);
        assert!(!is_virtual(&real) && !is_vpn(&real));
        assert!(!is_virtual(&wifi(2, [10, 0, 0, 2])));
        let tap = adapter(3, "Ethernet 2", "TAP-Windows Adapter V9", IF_TYPE_ETHERNET, [10, 8, 0, 2]);
        assert!(is_virtual(&tap));
        let docker = adapter(4, "vEthernet (WSL)", "Hyper-V Virtual Ethernet Adapter #2", IF_TYPE_ETHERNET, [172, 17, 0, 1]);
        assert!(is_virtual(&docker));
        let ppp = adapter(5, "Broadband", "WAN Miniport (PPPOE)", IF_TYPE_PPP, [10, 1, 1, 1]);
        assert!(is_vpn(&ppp));
        let attach = adapter(6, "Attachments", "Attachment Gigabit Ethernet", IF_TYPE_ETHERNET, [10, 1, 1, 2]);
        assert!(!is_virtual(&attach), "'tun'/'tap' must match whole tokens only");
    }

    #[test]
    fn os_name_switches_at_build_22000() {
        assert_eq!(os_name(19044), "Windows 10");
        assert_eq!(os_name(21999), "Windows 10");
        assert_eq!(os_name(22000), "Windows 11");
        assert_eq!(os_name(26200), "Windows 11");
    }

    #[test]
    fn live_identity_is_populated() {
        let id = identity();
        assert!(!id.computer_name.is_empty());
        assert!(id.windows_user.contains('\\'));
        assert!(id.os_build >= 19041);
        assert_eq!(id.computer_name, id.computer_name.to_uppercase());
    }

    #[test]
    fn live_local_ip_never_panics_and_is_cached() {
        let first = local_ipv4();
        let second = local_ipv4();
        assert_eq!(first.map(|s| s.ip), second.map(|s| s.ip));
    }
}
