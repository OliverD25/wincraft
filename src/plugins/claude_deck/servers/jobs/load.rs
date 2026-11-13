//! The probe's lines about jobs: `JOB` (a process group in a user unit or
//! a login session, under the home folder) and `UNIT` (what systemd knows
//! of a unit the PC asked about). A line of a broken form is skipped, and
//! nothing here can panic.

/// The longest command line kept, in characters.
const ARGS_MAX: usize = 300;
const NAME_MAX: usize = 120;

/// One job the server listed: a user unit, or a process tree in a login
/// session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawJob {
    /// The unit, like `verdict-archive-write.service`; None in a session.
    pub unit: Option<String>,
    /// The unit is started by a timer.
    pub timer: bool,
    /// The unit starts at boot (systemd's "enabled").
    pub enabled: bool,
    /// The main process: the oldest in the unit or tree.
    pub pid: u32,
    /// Seconds since the main process started.
    pub secs: u64,
    /// CPU of the whole unit or tree, in tenths of a percent.
    pub cpu10: u32,
    /// Resident memory of the whole unit or tree, in kB.
    pub rss_kb: u64,
    /// The main process's folder.
    pub cwd: String,
    /// The main process's command line, cut at `ARGS_MAX`.
    pub args: String,
}

/// What systemd knows of a unit, with its times in the PC's clock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitInfo {
    pub name: String,
    /// systemd's active state: `active`, `activating`, `inactive`, `failed`.
    pub active: String,
    /// systemd's result: `success`, `exit-code`, `signal` and others.
    pub result: String,
    /// The main process's exit status.
    pub status: Option<i32>,
    /// When the main process started and ended, in Unix ms.
    pub started: Option<u64>,
    pub ended: Option<u64>,
}

impl UnitInfo {
    /// The unit no longer runs.
    pub fn is_over(&self) -> bool {
        matches!(self.active.as_str(), "inactive" | "failed")
    }

    /// It ended well: systemd says success and the exit status is 0.
    pub fn ended_well(&self) -> bool {
        self.active != "failed" && self.result == "success" && self.status.unwrap_or(0) == 0
    }
}

fn clean(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// "12.3" as 123; None when it is not a number.
fn tenths(text: &str) -> Option<u32> {
    let (whole, part) = text.split_once('.').unwrap_or((text, "0"));
    let whole: u32 = whole.parse().ok()?;
    let first = part.chars().next().unwrap_or('0').to_digit(10)?;
    whole.checked_mul(10)?.checked_add(first)
}

/// A unit name the probe may print and be asked about.
pub fn plain_unit(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@' | ':'))
}

/// `JOB unit|- t|e|- pid secs cpu rss<TAB>cwd<TAB>args`.
pub fn job(rest: &str) -> Option<RawJob> {
    let mut parts = rest.splitn(3, '\t');
    let head = parts.next()?;
    let cwd = clean(parts.next()?, ARGS_MAX);
    let args = clean(parts.next().unwrap_or(""), ARGS_MAX);
    let mut words = head.split_whitespace();
    let unit = words.next()?;
    let flag = words.next()?;
    let pid = words.next()?.parse().ok()?;
    let secs = words.next()?.parse().ok()?;
    let cpu10 = tenths(words.next()?)?;
    let rss_kb = words.next()?.parse().ok()?;
    if words.next().is_some() || !cwd.starts_with('/') {
        return None;
    }
    let unit = match unit {
        "-" => None,
        name if plain_unit(name) => Some(name.to_string()),
        _ => return None,
    };
    Some(RawJob {
        timer: unit.is_some() && flag == "t",
        enabled: unit.is_some() && flag == "e",
        unit,
        pid,
        secs,
        cpu10,
        rss_kb,
        cwd,
        args,
    })
}

/// `@1794525606` as Unix ms in the PC's clock; None when empty.
fn stamp(value: &str, offset_ms: i64) -> Option<u64> {
    let secs: i64 = value.strip_prefix('@')?.parse().ok()?;
    u64::try_from(secs.checked_mul(1000)?.checked_add(offset_ms)?).ok()
}

/// `UNIT Key=value ...` as systemd's `show` prints it. `offset_ms` moves a
/// server time to the PC's clock. A unit systemd does not know is None.
pub fn unit(rest: &str, offset_ms: i64) -> Option<UnitInfo> {
    let mut info = UnitInfo::default();
    let mut loaded = false;
    for pair in rest.split_whitespace() {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "Id" if plain_unit(value) => info.name = value.to_string(),
            "LoadState" => loaded = value == "loaded",
            "ActiveState" => info.active = clean(value, 20),
            "Result" => info.result = clean(value, 20),
            "ExecMainStatus" => info.status = value.parse().ok(),
            "ExecMainStartTimestamp" => info.started = stamp(value, offset_ms),
            "ExecMainExitTimestamp" => info.ended = stamp(value, offset_ms),
            _ => {}
        }
    }
    (loaded && !info.name.is_empty()).then_some(info)
}
