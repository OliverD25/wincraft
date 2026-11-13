//! The read-only script ClaudeDeck sends to a server on ssh's standard
//! input (`ssh <name> sh -s`), and the ssh call itself. The script only
//! reads: the clock, `/proc/stat`, `/proc/meminfo`, the running Remote
//! Control units (`claude-rc@<folder>`), the folders of running `claude`
//! processes, and the end of each recent transcript under
//! `~/.claude/projects`; for the Servers tab also the `claude` processes,
//! the five busiest processes, systemd's units and Docker's containers;
//! and the jobs that run under the home folder, for "Work from the PC".
//! Nothing is installed or written on the server.

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
/// unit that failed or keeps restarting), `PROC`, `CLP pid cpu rss secs
/// folder<TAB>command` (a `claude` process), `BUSY pid cpu rss
/// name<TAB>command` (the five busiest), `SVC system|user unit active sub`
/// (failed units, the user's own units, and system units that run as the
/// user), `DOCK name<TAB>state<TAB>status` (every container; nothing when
/// Docker is missing or not allowed), `JOB unit|- t|e|- pid secs cpu
/// rss<TAB>cwd<TAB>args` (a job: one line per user unit, or per process
/// tree in a login session, whose folder is under the home folder; shells,
/// ssh, systemd, `claude` and the probe's own tools are skipped; `t` is a
/// unit a timer starts, `e` one that starts at boot; at most 40), `UNIT
/// Key=value ...` (systemd's data of each unit named in `units`), then per
/// transcript
/// `CHAT dir file mtime size` with the base64 of its end and `END`, or
/// `SAME dir file mtime size` when it has not changed since the last call,
/// or `READY dir mtime` for a Remote Control folder with no transcript yet
/// (an idle service writes nothing until a chat starts; the time is its
/// `bridge-pointer.json`'s); `DONE` last. `HOME` names the home folder, so
/// a Remote Control unit's folder is `$HOME/projects/<name>`.
/// `TOP` skips the probe's own processes (its `ps` and `awk`).
const BODY: &str = r#"export LC_ALL=C
now=$(date +%s)
echo "NOW $now"
echo "HOME $HOME"
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
ps -eo pid=,ppid=,pcpu=,comm= --sort=-pcpu 2>/dev/null | awk -v me=$$ '$1 != me && $2 != me { printf "TOP %d %s\n", $3, $4; exit }'
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
args() { tr '\0' ' ' < "/proc/$1/cmdline" 2>/dev/null | cut -c1-300; }
for p in $(pgrep -x claude 2>/dev/null); do
  d=$(readlink "/proc/$p/cwd" 2>/dev/null) || continue
  echo "PROC $d"
  s=$(ps -o pcpu=,rss=,etimes= -p "$p" 2>/dev/null) || continue
  set -- $s
  [ $# -eq 3 ] && printf 'CLP %s %s %s %s %s\t%s\n' "$p" "$1" "$2" "$3" "$d" "$(args "$p")"
done
ps -eo pid=,ppid=,pcpu=,rss=,comm= --sort=-pcpu 2>/dev/null | awk -v me=$$ '$1 != me && $2 != me { print $1, $3, $4, $5; if (++n == 5) exit }' | while read -r p c r n; do
  printf 'BUSY %s %s %s %s\t%s\n' "$p" "$c" "$r" "$n" "$(args "$p")"
done
own=$(ls "$HOME/.config/systemd/user" 2>/dev/null | grep '\.service$' | tr '\n' ' ')
systemctl --user list-units --all --type=service --no-legend --plain 2>/dev/null | while read -r u _ a s _; do
  b=$(printf '%s' "$u" | sed 's/@[^.]*\.service$/@.service/')
  case " $own " in *" $u "*|*" $b "*) echo "SVC user $u $a $s";; *) [ "$a" = failed ] && echo "SVC user $u $a $s";; esac
done
mine=$(grep -ls "^User=$(id -un)\$" /etc/systemd/system/*.service 2>/dev/null | sed 's|.*/||' | tr '\n' ' ')
[ -n "$mine" ] && systemctl list-units --all --no-legend --plain $mine 2>/dev/null | awk '{ print "SVC system", $1, $3, $4 }'
systemctl list-units --all --type=service --no-legend --plain 2>/dev/null | awk '$3 == "failed" || ($3 == "activating" && $4 == "auto-restart") { print "SVC system", $1, $3, $4 }'
timeout 5 docker ps -a --format '{{.Names}}\t{{.State}}\t{{.Status}}' </dev/null 2>/dev/null | head -40 | sed 's/^/DOCK /'
tab=$(printf '\t')
uid=$(id -u)
mgr=$(pgrep -u "$uid" -x systemd 2>/dev/null | head -n 1)
ps -u "$uid" -o pid=,ppid=,etimes=,pcpu=,rss=,comm= 2>/dev/null | while read -r p pp e c r n; do
  [ "$p" = "$$" ] && continue
  case $n in sshd|sshd-session|systemd|"(sd-pam)"|claude|ps|sleep|tmux*|screen|head|tail|cat|awk|sed|grep|cut|tr|sort|wc|base64|ls|stat|readlink|pgrep) continue;; esac
  case $n in bash|sh|dash|zsh|fish) [ "$pp" = 1 ] || [ "$pp" = "$mgr" ] || continue;; esac
  read -r g < "/proc/$p/cgroup" 2>/dev/null || continue
  case $g in */user.slice/*) ;; *) continue;; esac
  u=${g##*/}
  case $u in user@*|claude-rc@*|init.scope) continue;; *.service) ;; *.scope) u=-;; *) continue;; esac
  d=$(readlink "/proc/$p/cwd" 2>/dev/null) || continue
  case $d in "$HOME"|"$HOME"/*) ;; *) continue;; esac
  printf '%s %s %s %s %s %s\t%s\n' "$u" "$p" "$pp" "$e" "$c" "$r" "$d"
done | awk 'BEGIN { FS = "\t" }
{ split($1, a, " "); p = a[2]; u[p] = a[1]; par[p] = a[3]; et[p] = a[4] + 0; cpu[p] = a[5] + 0; rss[p] = a[6] + 0; cwd[p] = $2; o[++n] = p }
END {
  for (i = 1; i <= n; i++) {
    p = o[i]; k = u[p]
    if (k == "-") { q = p; while ((par[q] in u) && u[par[q]] == "-") q = par[q]; k = "-" q }
    if (!(k in m)) { m[k] = p; ks[++c] = k } else if (et[p] > et[m[k]]) m[k] = p
    cs[k] += cpu[p]; rs[k] += rss[p]
  }
  for (j = 1; j <= c && j <= 40; j++) { k = ks[j]; p = m[k]; printf "%s %s %d %.1f %d\t%s\n", (substr(k, 1, 1) == "-" ? "-" : k), p, et[p], cs[k], rs[k], cwd[p] }
}' | while IFS="$tab" read -r h d; do
  set -- $h
  f=-
  [ "$1" != - ] && f=$(systemctl --user show -p TriggeredBy -p UnitFileState "$1" 2>/dev/null | awk -F= '$1 == "TriggeredBy" && $2 != "" { t = 1 } $1 == "UnitFileState" && $2 == "enabled" { e = 1 } END { print t ? "t" : e ? "e" : "-" }')
  printf 'JOB %s %s %s %s %s %s\t%s\t%s\n' "$1" "$f" "$2" "$3" "$4" "$5" "$d" "$(args "$2")"
done
for u in $units; do
  s=$(systemctl --user show --timestamp=unix -p Id -p LoadState -p ActiveState -p Result -p ExecMainStatus -p ExecMainStartTimestamp -p ExecMainExitTimestamp "$u" 2>/dev/null | tr '\n' ' ')
  [ -n "$s" ] && echo "UNIT $s"
done
cd "$HOME/.claude/projects" 2>/dev/null || { echo DONE; exit 0; }
for d in */; do
  d=${d%/}
  f=
  for g in $(ls -t -- "$d"/*.jsonl 2>/dev/null); do [ -s "$g" ] && { f=$g; break; }; done
  keep=; isrc=
  for e in $rc; do case $d in *-projects-"$e") keep=1; isrc=1;; esac; done
  if [ -z "$f" ]; then
    if [ -n "$isrc" ]; then
      m=$(stat -c %Y -- "$d/bridge-pointer.json" 2>/dev/null || stat -c %Y -- "$d") && echo "READY $d $m"
    fi
    continue
  fi
  m=$(stat -c %Y -- "$f") || continue
  s=$(stat -c %s -- "$f") || continue
  [ $((now - m)) -le 86400 ] && keep=1
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
#[cfg(test)]
pub fn script(known: &[(String, u64, u64)]) -> String {
    script_with(known, &[])
}

/// The same, also told which user units to print systemd's data of.
pub fn script_with(known: &[(String, u64, u64)], units: &[String]) -> String {
    let names: Vec<String> = known
        .iter()
        .filter(|(dir, _, _)| plain_dir(dir))
        .map(|(dir, mtime, size)| format!("{dir}:{mtime}:{size}"))
        .collect();
    let units: Vec<&str> = units
        .iter()
        .map(String::as_str)
        .filter(|unit| super::jobs::load::plain_unit(unit))
        .take(40)
        .collect();
    format!(
        "known='{}'\nunits='{}'\n{BODY}",
        names.join(" "),
        units.join(" ")
    )
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
