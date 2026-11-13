//! What runs on a server, for the Servers tab: its `claude` processes,
//! its five busiest processes, and its services and containers. Read from
//! the probe's `CLP`, `BUSY`, `SVC` and `DOCK` lines; a line of a broken
//! form is skipped, and nothing here can panic.

/// The longest command line kept, in characters.
const COMMAND_MAX: usize = 300;
/// The longest name kept, in characters.
const NAME_MAX: usize = 80;

/// One process.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    /// CPU in tenths of a percent, as `ps` gives it.
    pub cpu10: u32,
    /// Resident memory, in kB.
    pub rss_kb: u64,
    /// Seconds since it started; only for a `claude` process.
    pub secs: Option<u64>,
    /// The folder it runs in; only for a `claude` process.
    pub folder: Option<String>,
    /// Its command name, like `dockerd`.
    pub name: String,
    /// Its whole command line, cut at `COMMAND_MAX`.
    pub command: String,
}

/// Where a service runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    System,
    User,
    Docker,
}

/// What a service or container does now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Stopped,
    Failed,
    Restarting,
}

impl State {
    /// A problem the tab counts.
    pub fn is_problem(self) -> bool {
        matches!(self, State::Failed | State::Restarting)
    }

    pub fn words(self) -> &'static str {
        match self {
            State::Running => "running",
            State::Stopped => "stopped",
            State::Failed => "failed",
            State::Restarting => "restarting",
        }
    }
}

/// A systemd unit or a Docker container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    pub kind: Kind,
    pub name: String,
    pub state: State,
    /// systemd's sub state, or Docker's status ("Up 3 days").
    pub detail: String,
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

/// `CLP pid cpu rss secs folder<TAB>command`.
pub fn claude_proc(rest: &str) -> Option<Proc> {
    let (head, command) = rest.split_once('\t').unwrap_or((rest, ""));
    let mut parts = head.splitn(5, ' ');
    let pid = parts.next()?.parse().ok()?;
    let cpu10 = tenths(parts.next()?)?;
    let rss_kb = parts.next()?.parse().ok()?;
    let secs = parts.next()?.parse().ok()?;
    let folder = clean(parts.next()?, COMMAND_MAX);
    if !folder.starts_with('/') {
        return None;
    }
    Some(Proc {
        pid,
        cpu10,
        rss_kb,
        secs: Some(secs),
        folder: Some(folder),
        name: "claude".to_string(),
        command: clean(command, COMMAND_MAX),
    })
}

/// `BUSY pid cpu rss name<TAB>command`.
pub fn busy_proc(rest: &str) -> Option<Proc> {
    let (head, command) = rest.split_once('\t').unwrap_or((rest, ""));
    let mut parts = head.splitn(4, ' ');
    let pid = parts.next()?.parse().ok()?;
    let cpu10 = tenths(parts.next()?)?;
    let rss_kb = parts.next()?.parse().ok()?;
    let name = clean(parts.next()?, NAME_MAX);
    if name.is_empty() {
        return None;
    }
    let command = clean(command, COMMAND_MAX);
    Some(Proc {
        pid,
        cpu10,
        rss_kb,
        secs: None,
        folder: None,
        command: if command.is_empty() {
            name.clone()
        } else {
            command
        },
        name,
    })
}

/// systemd's active and sub state as one of the four.
fn unit_state(active: &str, sub: &str) -> State {
    match (active, sub) {
        ("failed", _) => State::Failed,
        (_, "auto-restart") => State::Restarting,
        ("inactive", _) => State::Stopped,
        _ => State::Running,
    }
}

/// `SVC system|user unit active sub`.
pub fn unit(rest: &str) -> Option<Service> {
    let mut parts = rest.split_whitespace();
    let kind = match parts.next()? {
        "system" => Kind::System,
        "user" => Kind::User,
        _ => return None,
    };
    let name = clean(parts.next()?, NAME_MAX);
    let active = parts.next()?;
    let sub = parts.next()?;
    if name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(Service {
        kind,
        name,
        state: unit_state(active, sub),
        detail: clean(sub, NAME_MAX),
    })
}

/// Exit codes of a container someone stopped: 0, Ctrl+C, SIGKILL and
/// SIGTERM (`docker stop`).
const STOPPED_CODES: [&str; 4] = ["0", "130", "137", "143"];

/// `DOCK name<TAB>state<TAB>status`.
pub fn container(rest: &str) -> Option<Service> {
    let mut parts = rest.split('\t');
    let name = clean(parts.next()?, NAME_MAX);
    let state = parts.next()?.trim();
    let status = clean(parts.next().unwrap_or(""), NAME_MAX);
    if name.is_empty() {
        return None;
    }
    let code = status
        .strip_prefix("Exited (")
        .and_then(|after| after.split_once(')'))
        .map(|(code, _)| code);
    let state = match state {
        "running" | "paused" => State::Running,
        "restarting" => State::Restarting,
        "dead" => State::Failed,
        "exited" if code.is_some_and(|code| !STOPPED_CODES.contains(&code)) => State::Failed,
        "exited" | "created" | "removing" => State::Stopped,
        _ => return None,
    };
    Some(Service {
        kind: Kind::Docker,
        name,
        state,
        detail: status,
    })
}

/// Adds a service unless one of the same kind and name is there.
pub fn add_service(services: &mut Vec<Service>, service: Service) {
    let same = services
        .iter()
        .any(|s| s.kind == service.kind && s.name == service.name);
    if !same {
        services.push(service);
    }
}
