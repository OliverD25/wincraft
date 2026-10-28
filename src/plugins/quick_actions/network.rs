//! The PC's own IPv4 addresses, read from the network adapters with
//! GetAdaptersAddresses. Nothing here goes out to the network: no public
//! address, no outside service (Principle 5).

use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
    IP_ADAPTER_ADDRESSES_LH,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::core::{from_wide_ptr, wide};

const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_WIFI: u32 = 71;
const IF_TYPE_LOOPBACK: u32 = 24;
const ERROR_BUFFER_OVERFLOW: u32 = 111;

/// What kind of connection an address belongs to, in the order the
/// inspector lists them: the first one is what Enter copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    WiFi,
    Ethernet,
    Wsl,
    Other,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::WiFi => "Wi-Fi",
            Kind::Ethernet => "Ethernet",
            Kind::Wsl => "WSL",
            Kind::Other => "Other",
        }
    }
}

/// One address of the PC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub kind: Kind,
    /// The adapter's name as Windows shows it, such as "Wi-Fi 2".
    pub adapter: String,
    pub ip: String,
}

/// The kind of an adapter from its type and its name; None for one that
/// is not worth listing, such as loopback.
pub fn classify(if_type: u32, name: &str) -> Option<Kind> {
    let lower = name.to_lowercase();
    match if_type {
        IF_TYPE_LOOPBACK => None,
        IF_TYPE_WIFI => Some(Kind::WiFi),
        _ if lower.starts_with("vethernet") && lower.contains("wsl") => Some(Kind::Wsl),
        // Other Hyper-V switches are virtual, not a way into the PC.
        _ if lower.starts_with("vethernet") => Some(Kind::Other),
        IF_TYPE_ETHERNET => Some(Kind::Ethernet),
        _ => Some(Kind::Other),
    }
}

/// An address worth listing: not a link-local 169.254 one Windows makes up
/// when no network answered.
pub fn usable(ip: [u8; 4]) -> bool {
    ip != [0, 0, 0, 0] && !(ip[0] == 169 && ip[1] == 254) && ip[0] != 127
}

/// Sorts by kind and keeps the adapters' own order within a kind.
pub fn ordered(mut found: Vec<Address>) -> Vec<Address> {
    found.sort_by_key(|address| address.kind);
    found
}

pub fn read() -> Vec<Address> {
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    let mut buffer: Vec<u64> = Vec::new();
    for _ in 0..3 {
        buffer = vec![0u64; (size as usize).div_ceil(8)];
        let status = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET),
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        match status {
            0 => break,
            ERROR_BUFFER_OVERFLOW => continue,
            _ => return Vec::new(),
        }
    }
    let mut found = Vec::new();
    let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !adapter.is_null() {
        let current = unsafe { &*adapter };
        adapter = current.Next;
        if current.OperStatus != IfOperStatusUp {
            continue;
        }
        let name = if current.FriendlyName.is_null() {
            String::new()
        } else {
            unsafe { from_wide_ptr(current.FriendlyName) }
        };
        let Some(kind) = classify(current.IfType, &name) else {
            continue;
        };
        let mut unicast = current.FirstUnicastAddress;
        while !unicast.is_null() {
            let address = unsafe { &*unicast };
            unicast = address.Next;
            let socket = address.Address.lpSockaddr;
            if socket.is_null() || unsafe { (*socket).sa_family } != AF_INET {
                continue;
            }
            let ip =
                unsafe { (*(socket as *const SOCKADDR_IN)).sin_addr.S_un.S_addr }.to_ne_bytes();
            if usable(ip) {
                found.push(Address {
                    kind,
                    adapter: name.clone(),
                    ip: format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]),
                });
            }
        }
    }
    ordered(found)
}

/// Empties Windows' list of names it has looked up. DnsFlushResolverCache
/// is what `ipconfig /flushdns` calls; it is not in the SDK headers, so it
/// is found by name.
pub fn flush_dns() -> Result<(), String> {
    let library = unsafe { LoadLibraryW(wide("dnsapi.dll").as_ptr()) };
    if library.is_null() {
        return Err("Windows has no DNS client library".to_string());
    }
    let Some(flush) =
        (unsafe { GetProcAddress(library, c"DnsFlushResolverCache".as_ptr().cast()) })
    else {
        return Err("Windows has no way to clear the DNS cache here".to_string());
    };
    let flush: unsafe extern "system" fn() -> i32 = unsafe { std::mem::transmute(flush) };
    if unsafe { flush() } != 0 {
        Ok(())
    } else {
        Err("Windows refused to clear the DNS cache".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(kind: Kind, ip: &str) -> Address {
        Address {
            kind,
            adapter: kind.name().to_string(),
            ip: ip.to_string(),
        }
    }

    #[test]
    fn adapters_are_sorted_into_wifi_ethernet_and_wsl() {
        assert_eq!(classify(IF_TYPE_WIFI, "Wi-Fi"), Some(Kind::WiFi));
        assert_eq!(
            classify(IF_TYPE_ETHERNET, "Ethernet 2"),
            Some(Kind::Ethernet)
        );
        assert_eq!(
            classify(IF_TYPE_ETHERNET, "vEthernet (WSL (Hyper-V firewall))"),
            Some(Kind::Wsl)
        );
        assert_eq!(
            classify(IF_TYPE_ETHERNET, "vEthernet (Default Switch)"),
            Some(Kind::Other)
        );
        assert_eq!(
            classify(IF_TYPE_LOOPBACK, "Loopback Pseudo-Interface 1"),
            None
        );
        assert_eq!(classify(53, "Tailscale"), Some(Kind::Other));
        for kind in [Kind::WiFi, Kind::Ethernet, Kind::Wsl, Kind::Other] {
            assert!(!kind.name().is_empty());
        }
    }

    #[test]
    fn made_up_and_loopback_addresses_are_left_out() {
        assert!(usable([192, 168, 1, 20]));
        assert!(usable([172, 28, 160, 1]));
        assert!(!usable([169, 254, 3, 4]));
        assert!(!usable([127, 0, 0, 1]));
        assert!(!usable([0, 0, 0, 0]));
    }

    #[test]
    fn wifi_comes_first_and_the_adapters_keep_their_order() {
        let sorted = ordered(vec![
            address(Kind::Wsl, "172.28.160.1"),
            address(Kind::Ethernet, "10.0.0.5"),
            address(Kind::Other, "100.64.0.1"),
            address(Kind::WiFi, "192.168.1.20"),
            address(Kind::Ethernet, "10.0.0.6"),
        ]);
        let ips: Vec<&str> = sorted.iter().map(|a| a.ip.as_str()).collect();
        assert_eq!(
            ips,
            [
                "192.168.1.20",
                "10.0.0.5",
                "10.0.0.6",
                "172.28.160.1",
                "100.64.0.1"
            ]
        );
    }
}
