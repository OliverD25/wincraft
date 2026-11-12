//! The read-only script ClaudeDeck sends to a server on ssh's standard
//! input (`ssh <name> sh -s`), and the ssh call itself. The script only
//! reads: the clock, `/proc/stat`, `/proc/meminfo`, the running Remote
//! Control units (`claude-rc@<folder>`), the folders of running `claude`
//! processes, and the end of each recent transcript under
//! `~/.claude/projects`. Nothing is installed or written on the server.

use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

/// The most of one transcript the script sends.
pub const TAIL_BYTES: u64 = 65_536;
/// ssh gives up connecting after this many seconds.
const CONNECT_TIMEOUT: &str = "ConnectTimeout=6";
/// The whole call is stopped after this long.
pub const KILL_AFTER: Duration = Duration::from_secs(20);

/// The script, after the line that names the transcripts already read.
/// The newest transcript that is not empty is taken: the app often
/// leaves an empty one newest (seen on a server, 2026-11-12). Unit names
/// are used as they are; unescaping them turned `-` into `/`.
/// Lines it prints: `NOW`, `CPU`, `MEM`, `UP` (seconds), `REBOOT` (a
/// restart is needed), `DISK` (free and total kB of `/`), `TOP` (the
/// busiest process: its CPU % and name), `RC`, `RCFAIL` (a Remote Control
/// unit that failed or keeps restarting), `PROC`, then per transcript
/// `CHAT dir file mtime size` with the base64 of its end and `END`, or
/// `SAME dir file mtime size` when it has not changed since the last call;
/// `DONE` last.
const BODY: &str = r#"export LC_ALL=C
now=$(date +%s)
echo "NOW $now"
cpu() { read -r _ u n s i w q sq st _ < /proc/stat; echo "$((u+n+s+i+w+q+sq+st)) $((i+w))"; }
set -- $(cpu); t1=$1; i1=$2
sleep 0.5
set -- $(cpu); t2=$1; i2=$2
dt=$((t2 - t1)); di=$((i2 - i1))
[ "$dt" -gt 0 ] && echo "CPU $(( (100 * (dt - di)) / dt ))"
mt=; ma=
while read -r k v _; do case $k in MemTotal:) mt=$v;; MemAvailable:) ma=$v;; esac; done < /proc/meminfo
[ -n "$mt" ] && [ -n "$ma" ] && echo "MEM $mt $ma"
read -r up _ < /proc/uptime && echo "UP ${up%.*}"
[ -e /var/run/reboot-required ] && echo "REBOOT 1"
df -Pk / 2>/dev/null | awk 'NR == 2 { print "DISK", $4, $2 }'
ps -eo pcpu=,comm= --sort=-pcpu 2>/dev/null | awk 'NR == 1 { printf "TOP %d %s\n", $1, $2 }'
rc=
for u in $(systemctl --user list-units --state=active --no-legend --plain 'claude-rc@*' 2>/dev/null | cut -d' ' -f1); do
  f=${u#claude-rc@}; f=${f%.service}
  echo "RC $f"
  rc="$rc $(printf '%s' "$f" | sed 's/[^A-Za-z0-9]/-/g')"
done
for u in $(systemctl --user list-units --all --no-legend --plain 'claude-rc@*' 2>/dev/null | awk '$3 == "failed" || ($3 == "activating" && $4 == "auto-restart") { print $1 }'); do
  f=${u#claude-rc@}; f=${f%.service}
  echo "RCFAIL $f"
  rc="$rc $(printf '%s' "$f" | sed 's/[^A-Za-z0-9]/-/g')"
done
for p in $(pgrep -x claude 2>/dev/null); do d=$(readlink "/proc/$p/cwd" 2>/dev/null) && echo "PROC $d"; done
cd "$HOME/.claude/projects" 2>/dev/null || { echo DONE; exit 0; }
for d in */; do
  d=${d%/}
  f=
  for g in $(ls -t -- "$d"/*.jsonl 2>/dev/null); do [ -s "$g" ] && { f=$g; break; }; done
  [ -n "$f" ] || continue
  m=$(stat -c %Y -- "$f") || continue
  s=$(stat -c %s -- "$f") || continue
  keep=
  [ $((now - m)) -le 86400 ] && keep=1
  for e in $rc; do case $d in *-projects-"$e") keep=1;; esac; done
  [ -n "$keep" ] || continue
  case " $known " in *" $d:$m:$s "*) echo "SAME $d ${f##*/} $m $s"; continue;; esac
  echo "CHAT $d ${f##*/} $m $s"
  tail -c 65536 -- "$f" | base64
  echo END
done
echo DONE
"#;

/// A transcript folder name the script may be told about: Claude makes
/// them from a path with every other character turned into `-`.
pub fn plain_dir(dir: &str) -> bool {
    !dir.is_empty()
        && dir
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The script, told which transcripts (folder, mtime, size) it need not
/// send again.
pub fn script(known: &[(String, u64, u64)]) -> String {
    let names: Vec<String> = known
        .iter()
        .filter(|(dir, _, _)| plain_dir(dir))
        .map(|(dir, mtime, size)| format!("{dir}:{mtime}:{size}"))
        .collect();
    format!("known='{}'\n{BODY}", names.join(" "))
}

/// Windows' own ssh, which reads the user's `~/.ssh/config`; else the
/// one on PATH.
pub fn ssh_program() -> PathBuf {
    let windows = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join(r"System32\OpenSSH\ssh.exe");
    if windows.is_file() {
        windows
    } else {
        PathBuf::from("ssh.exe")
    }
}

/// ssh's arguments for one call: never a password prompt, a short
/// connect time, and the script read from standard input.
pub fn ssh_args(host: &str) -> Vec<String> {
    [
        "-o",
        "BatchMode=yes",
        "-o",
        CONNECT_TIMEOUT,
        "--",
        host,
        "sh",
        "-s",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect()
}

/// Runs the script on the server and returns what it printed. Stopped
/// after `KILL_AFTER`; no console window shows.
pub fn run(host: &str, script: &str) -> Result<String, String> {
    let mut child = Command::new(ssh_program())
        .args(ssh_args(host))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|err| format!("could not start ssh: {err}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A server that is down may close the pipe early; the reply tells.
        let _ = stdin.write_all(script.as_bytes());
    }
    let mut stdout = child.stdout.take().ok_or("no output pipe")?;
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stdout.read_to_end(&mut bytes);
        let _ = send.send(read.map(|_| bytes));
    });
    let reply = receive.recv_timeout(KILL_AFTER);
    if reply.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    match reply {
        Ok(Ok(bytes)) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Ok(Err(err)) => Err(format!("could not read ssh's output: {err}")),
        Err(_) => Err("ssh took too long".to_string()),
    }
}
