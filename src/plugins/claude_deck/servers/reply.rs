//! Reading the probe's reply. Every byte of it is treated as data: a line
//! of an unknown or broken form is skipped, a transcript whose base64
//! does not decode is left out, and nothing here can panic.

use super::load::{self, Proc, Service};

/// One transcript the server listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawChat {
    /// Its folder under `~/.claude/projects`.
    pub dir: String,
    pub file: String,
    /// Last change, in the server's Unix seconds.
    pub mtime: u64,
    pub size: u64,
    /// The end of the file; None when the server said it did not change.
    pub tail: Option<Vec<u8>>,
}

/// What one probe call told.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    /// The server's clock, in Unix seconds.
    pub now: u64,
    pub cpu: Option<u8>,
    /// MemTotal and MemAvailable, in kB.
    pub mem: Option<(u64, u64)>,
    /// The folders of the running Remote Control units.
    pub rc: Vec<String>,
    /// The folders of Remote Control units that failed or keep restarting.
    pub rc_failed: Vec<String>,
    /// The folders `claude` processes run in.
    pub procs: Vec<String>,
    pub chats: Vec<RawChat>,
    pub health: Health,
    /// The server's home folder.
    pub home: Option<String>,
    /// Remote Control folders with no transcript yet, with the time of
    /// their `bridge-pointer.json` (server seconds).
    pub ready: Vec<(String, u64)>,
    /// The running `claude` processes.
    pub claude: Vec<Proc>,
    /// The five busiest processes of any kind, busiest first.
    pub busiest: Vec<Proc>,
    /// Failed services, the user's own, and every Docker container.
    pub services: Vec<Service>,
}

/// How the server itself is, for the hover panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Health {
    /// Seconds since it started.
    pub up: Option<u64>,
    /// `/var/run/reboot-required` exists.
    pub reboot: bool,
    /// Free and total kB of `/`.
    pub disk: Option<(u64, u64)>,
    /// The busiest process: its CPU % and command name.
    pub top: Option<(u32, String)>,
}

impl Reply {
    /// RAM in use, in percent.
    pub fn ram(&self) -> Option<u8> {
        let (total, available) = self.mem?;
        if total == 0 || available > total {
            return None;
        }
        Some(((total - available) * 100 / total) as u8)
    }
}

/// The reply, or None when it is not a whole reply: no `NOW` line or no
/// `DONE` line (the server is down, ssh failed, or the call was cut).
pub fn parse(text: &str) -> Option<Reply> {
    let mut reply = Reply::default();
    let mut has_now = false;
    let mut done = false;
    let mut open: Option<(RawChat, String)> = None;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some((mut chat, mut base64)) = open.take() {
            if line == "END" {
                chat.tail = decode(&base64);
                if chat.tail.is_some() {
                    reply.chats.push(chat);
                }
            } else if line.len() <= 200 && line.bytes().all(is_base64_byte) {
                base64.push_str(line);
                open = Some((chat, base64));
            }
            // Any other line ends a broken block: it is dropped.
            continue;
        }
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "NOW" => {
                if let Ok(now) = rest.trim().parse() {
                    reply.now = now;
                    has_now = true;
                }
            }
            "CPU" => reply.cpu = rest.trim().parse::<u8>().ok().filter(|cpu| *cpu <= 100),
            "MEM" => {
                let mut parts = rest.split_whitespace().map(str::parse::<u64>);
                if let (Some(Ok(total)), Some(Ok(available))) = (parts.next(), parts.next()) {
                    reply.mem = Some((total, available));
                }
            }
            "RC" if !rest.trim().is_empty() => reply.rc.push(rest.trim().to_string()),
            "RCFAIL" if !rest.trim().is_empty() => reply.rc_failed.push(rest.trim().to_string()),
            "PROC" if !rest.trim().is_empty() => reply.procs.push(rest.trim().to_string()),
            "CHAT" => open = raw_chat(rest).map(|chat| (chat, String::new())),
            "SAME" => reply.chats.extend(raw_chat(rest)),
            "UP" => reply.health.up = rest.trim().parse().ok(),
            "REBOOT" => reply.health.reboot = rest.trim() == "1",
            "DISK" => {
                let mut parts = rest.split_whitespace().map(str::parse::<u64>);
                if let (Some(Ok(free)), Some(Ok(total))) = (parts.next(), parts.next()) {
                    reply.health.disk = (free <= total).then_some((free, total));
                }
            }
            "TOP" => {
                let mut parts = rest.split_whitespace();
                if let (Some(Ok(cpu)), Some(name)) =
                    (parts.next().map(str::parse::<u32>), parts.next())
                {
                    let name: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
                    reply.health.top = Some((cpu, name));
                }
            }
            "HOME" if rest.trim().starts_with('/') => {
                reply.home = Some(rest.trim().trim_end_matches('/').to_string());
            }
            "READY" => {
                let mut parts = rest.split_whitespace();
                if let (Some(dir), Some(Ok(mtime)), None) = (
                    parts.next(),
                    parts.next().map(str::parse::<u64>),
                    parts.next(),
                ) {
                    reply.ready.push((dir.to_string(), mtime));
                }
            }
            "CLP" => reply.claude.extend(load::claude_proc(rest)),
            "BUSY" if reply.busiest.len() < 5 => reply.busiest.extend(load::busy_proc(rest)),
            "SVC" => {
                if let Some(service) = load::unit(rest) {
                    load::add_service(&mut reply.services, service);
                }
            }
            "DOCK" => {
                if let Some(service) = load::container(rest) {
                    load::add_service(&mut reply.services, service);
                }
            }
            "DONE" => done = true,
            _ => {}
        }
    }
    (has_now && done).then_some(reply)
}

/// `dir file mtime size`.
fn raw_chat(rest: &str) -> Option<RawChat> {
    let mut parts = rest.split_whitespace();
    let (dir, file, mtime, size) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    Some(RawChat {
        dir: dir.to_string(),
        file: file.to_string(),
        mtime: mtime.parse().ok()?,
        size: size.parse().ok()?,
        tail: None,
    })
}

fn is_base64_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
}

fn value(byte: u8) -> Option<u32> {
    Some(match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    } as u32)
}

/// Standard base64 with padding; None when it is not.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let quads = bytes.len() / 4;
    for (at, quad) in bytes.chunks(4).enumerate() {
        let last = at + 1 == quads;
        let pad = quad.iter().rev().take_while(|b| **b == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n: u32 = 0;
        for byte in &quad[..4 - pad] {
            n = (n << 6) | value(*byte)?;
        }
        n <<= 6 * pad as u32;
        let three = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&three[..3 - pad]);
    }
    Some(out)
}
