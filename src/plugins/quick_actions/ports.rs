//! The programs that listen on a TCP port, from GetExtendedTcpTable, and
//! ending one of them. The pure part decides which rows to offer: never
//! WinCraft itself, never the System Idle Process (PID 0) or System (4).

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

/// One listening socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Listener {
    pub port: u16,
    pub pid: u32,
    pub v6: bool,
}

/// One row of the list: a program on a port, over IPv4, IPv6 or both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    pub port: u16,
    pub pid: u32,
    pub name: String,
    pub v4: bool,
    pub v6: bool,
}

impl Port {
    pub fn id(&self) -> String {
        format!("{}:{}", self.pid, self.port)
    }

    pub fn protocols(&self) -> &'static str {
        match (self.v4, self.v6) {
            (true, true) => "IPv4 and IPv6",
            (false, true) => "IPv6",
            _ => "IPv4",
        }
    }
}

/// "1234:3000" back into the PID and the port.
pub fn parse_id(id: &str) -> Option<(u32, u16)> {
    let (pid, port) = id.split_once(':')?;
    Some((pid.parse().ok()?, port.parse().ok()?))
}

/// Whether a PID may be ended from the list.
pub fn may_end(pid: u32, own: u32) -> bool {
    pid != 0 && pid != 4 && pid != own
}

/// One row per program and port, sorted by port, without the processes
/// that must never be ended.
pub fn rows(listeners: &[Listener], own: u32, name: &dyn Fn(u32) -> String) -> Vec<Port> {
    let mut rows: Vec<Port> = Vec::new();
    for listener in listeners.iter().filter(|l| may_end(l.pid, own)) {
        if let Some(row) = rows
            .iter_mut()
            .find(|row| row.port == listener.port && row.pid == listener.pid)
        {
            row.v4 |= !listener.v6;
            row.v6 |= listener.v6;
            continue;
        }
        rows.push(Port {
            port: listener.port,
            pid: listener.pid,
            name: String::new(),
            v4: !listener.v6,
            v6: listener.v6,
        });
    }
    rows.sort_by_key(|row| (row.port, row.pid));
    for row in &mut rows {
        row.name = name(row.pid);
    }
    rows
}

/// The table for one address family, as raw bytes.
fn table(family: u16) -> Option<Vec<u64>> {
    let mut size = 0u32;
    let mut buffer: Vec<u64> = Vec::new();
    for _ in 0..4 {
        let status = unsafe {
            GetExtendedTcpTable(
                if buffer.is_empty() {
                    std::ptr::null_mut()
                } else {
                    buffer.as_mut_ptr().cast()
                },
                &mut size,
                0,
                u32::from(family),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        match status {
            0 if !buffer.is_empty() => return Some(buffer),
            0 | ERROR_INSUFFICIENT_BUFFER => {
                buffer = vec![0u64; (size as usize).div_ceil(8) + 1];
            }
            _ => return None,
        }
    }
    None
}

fn port_of(raw: u32) -> u16 {
    u16::from_be(raw as u16)
}

pub fn read() -> Vec<Listener> {
    let mut found = Vec::new();
    if let Some(buffer) = table(AF_INET) {
        let count = unsafe { *(buffer.as_ptr() as *const u32) } as usize;
        let first = unsafe { (buffer.as_ptr() as *const u8).add(4) } as *const MIB_TCPROW_OWNER_PID;
        for index in 0..count {
            let row = unsafe { std::ptr::read_unaligned(first.add(index)) };
            found.push(Listener {
                port: port_of(row.dwLocalPort),
                pid: row.dwOwningPid,
                v6: false,
            });
        }
    }
    if let Some(buffer) = table(AF_INET6) {
        let count = unsafe { *(buffer.as_ptr() as *const u32) } as usize;
        let first =
            unsafe { (buffer.as_ptr() as *const u8).add(4) } as *const MIB_TCP6ROW_OWNER_PID;
        for index in 0..count {
            let row = unsafe { std::ptr::read_unaligned(first.add(index)) };
            found.push(Listener {
                port: port_of(row.dwLocalPort),
                pid: row.dwOwningPid,
                v6: true,
            });
        }
    }
    found
}

/// Ends the process at once, as Task Manager's End task does.
pub fn end(pid: u32) -> Result<(), String> {
    let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if process.is_null() {
        return Err(refusal(unsafe { GetLastError() }));
    }
    let ended = unsafe { TerminateProcess(process, 1) } != 0;
    let error = unsafe { GetLastError() };
    unsafe { CloseHandle(process) };
    if ended {
        Ok(())
    } else {
        Err(refusal(error))
    }
}

fn refusal(error: u32) -> String {
    if error == ERROR_ACCESS_DENIED {
        "Windows refused: the program runs as administrator or belongs to Windows".to_string()
    } else {
        format!("Windows could not end it (error {error})")
    }
}

/// The text a refused end gives, for the fake PC.
#[cfg(test)]
pub fn tests_refusal() -> String {
    refusal(ERROR_ACCESS_DENIED)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listen(port: u16, pid: u32, v6: bool) -> Listener {
        Listener { port, pid, v6 }
    }

    #[test]
    fn rows_join_ipv4_and_ipv6_and_leave_out_the_system_and_wincraft() {
        let listeners = [
            listen(3000, 50, false),
            listen(3000, 50, true),
            listen(135, 4, false),
            listen(445, 0, true),
            listen(8090, 99, false),
            listen(80, 60, true),
        ];
        let rows = rows(&listeners, 99, &|pid| format!("app{pid}.exe"));
        let shown: Vec<(u16, u32, &str)> = rows
            .iter()
            .map(|row| (row.port, row.pid, row.protocols()))
            .collect();
        assert_eq!(shown, [(80, 60, "IPv6"), (3000, 50, "IPv4 and IPv6")]);
        assert_eq!(rows[1].name, "app50.exe");
        assert_eq!(rows[1].id(), "50:3000");
    }

    #[test]
    fn a_row_id_reads_back_and_only_safe_pids_may_end() {
        assert_eq!(parse_id("50:3000"), Some((50, 3000)));
        assert_eq!(parse_id("x:3000"), None);
        assert_eq!(parse_id("50"), None);
        assert!(may_end(50, 99));
        assert!(!may_end(0, 99));
        assert!(!may_end(4, 99));
        assert!(!may_end(99, 99));
        assert_eq!(port_of(u32::from(3000u16.to_be())), 3000);
        assert!(refusal(ERROR_ACCESS_DENIED).contains("administrator"));
        assert!(refusal(87).contains("87"));
    }
}
