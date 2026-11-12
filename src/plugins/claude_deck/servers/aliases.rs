//! The servers the user listed, and every other name ssh knows them by.
//! A chat may call a server by any name on its `Host` line in
//! `%USERPROFILE%\.ssh\config`; all of them mean the same server.

use std::collections::HashMap;
use std::path::PathBuf;

/// The servers from the setting: a comma or space list of ssh names.
/// A name must be a plain host name; one that starts with `-` would be
/// read by ssh as an option, so it is left out.
pub fn parse_servers(text: &str) -> Vec<String> {
    let mut servers: Vec<String> = Vec::new();
    for name in text.split(|c: char| c == ',' || c.is_whitespace()) {
        let name = name.trim();
        let plain = !name.is_empty()
            && !name.starts_with(['-', '.'])
            && name.len() <= 64
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
        if plain && !servers.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            servers.push(name.to_string());
        }
    }
    servers
}

/// What a server's group is called: its ssh name without a leading `rde`.
pub fn group_name(server: &str) -> String {
    match server.strip_prefix("rde") {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => server.to_string(),
    }
}

/// Every name of each listed server, in lower case, to the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Aliases {
    map: HashMap<String, String>,
}

impl Aliases {
    /// The names from the `Host` lines of `config` (patterns with `*` or
    /// `?` and negated names are skipped) that share a line with a listed
    /// server. A server with no line is known only by its own name.
    pub fn build(servers: &[String], config: Option<&str>) -> Self {
        let mut map = HashMap::new();
        for server in servers {
            map.insert(server.to_ascii_lowercase(), server.clone());
        }
        for names in config.map(host_lines).unwrap_or_default() {
            let Some(server) = servers
                .iter()
                .find(|server| names.iter().any(|n| n.eq_ignore_ascii_case(server)))
            else {
                continue;
            };
            for name in names {
                map.entry(name.to_ascii_lowercase())
                    .or_insert_with(|| server.clone());
            }
        }
        Aliases { map }
    }

    /// Reads the user's ssh config; a missing file means no other names.
    pub fn load(servers: &[String]) -> Self {
        let config = config_path().and_then(|path| std::fs::read_to_string(path).ok());
        Self::build(servers, config.as_deref())
    }

    /// The listed server this name means, if any.
    pub fn server_of(&self, host: &str) -> Option<&str> {
        self.map.get(&host.to_ascii_lowercase()).map(String::as_str)
    }
}

fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")?;
    Some(PathBuf::from(home).join(".ssh").join("config"))
}

/// The plain names of each `Host` line.
fn host_lines(config: &str) -> Vec<Vec<String>> {
    config
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (keyword, rest) = line.split_once(|c: char| c.is_whitespace() || c == '=')?;
            if !keyword.eq_ignore_ascii_case("host") {
                return None;
            }
            let names: Vec<String> = rest
                .split(|c: char| c.is_whitespace() || c == '=')
                .map(|name| name.trim_matches('"'))
                .take_while(|name| !name.starts_with('#'))
                .filter(|name| {
                    !name.is_empty() && !name.contains(['*', '?']) && !name.starts_with('!')
                })
                .map(str::to_string)
                .collect();
            (!names.is_empty()).then_some(names)
        })
        .collect()
}
