//! "Detailed log: Save a report": the newest detailed log file, the normal
//! log and the one before it, config.json and the last session header,
//! copied into one dated folder the user can hand over. The folder sits in
//! `reports` under the detailed log's folder, so "Delete the files" takes
//! the reports too.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::writer;

/// The reports' folder inside the detailed log's folder.
pub fn reports_dir(detailed: &Path) -> PathBuf {
    detailed.join("reports")
}

/// What one report holds, where it comes from.
pub struct Sources {
    /// The detailed log's folder.
    pub detailed: PathBuf,
    /// wincraft.log; wincraft.prev.log is taken from next to it.
    pub log: PathBuf,
    pub config: PathBuf,
}

/// Copies everything into `reports/report-<stamp>` and returns that folder.
/// A source that does not exist is left out; the folder always holds a
/// `report.txt` that lists what was copied.
pub fn save(sources: &Sources, stamp: &str) -> std::io::Result<PathBuf> {
    let folder = reports_dir(&sources.detailed).join(format!("report-{stamp}"));
    fs::create_dir_all(&folder)?;
    let mut copied = Vec::new();
    let newest = writer::files_in(&sources.detailed).pop();
    let previous_log = crate::core::logging::previous_path(&sources.log);
    let wanted: Vec<&Path> = newest
        .iter()
        .map(PathBuf::as_path)
        .chain([
            sources.log.as_path(),
            previous_log.as_path(),
            sources.config.as_path(),
        ])
        .collect();
    for source in wanted {
        let Some(name) = source.file_name() else {
            continue;
        };
        if fs::copy(source, folder.join(name)).is_ok() {
            copied.push(name.to_string_lossy().into_owned());
        }
    }
    let header = newest
        .as_deref()
        .map(last_header)
        .unwrap_or_else(|| "No detailed log file yet.\n".to_string());
    fs::write(folder.join("header.txt"), &header)?;
    let mut listing = String::from(
        "A WinCraft report. It may hold window titles and text typed in the palette.\n\nFiles:\n",
    );
    for name in copied
        .iter()
        .chain(std::iter::once(&"header.txt".to_string()))
    {
        listing.push_str(&format!("- {name}\n"));
    }
    fs::write(folder.join("report.txt"), listing)?;
    Ok(folder)
}

/// The last session header in a detailed log: the last "=== ON" line and
/// the header lines after it.
pub fn last_header(path: &Path) -> String {
    let Ok(file) = fs::File::open(path) else {
        return String::new();
    };
    header_of(BufReader::new(file).lines().map_while(Result::ok))
}

pub(crate) fn header_of(lines: impl Iterator<Item = String>) -> String {
    let mut block = String::new();
    for line in lines {
        if line.contains("] === ON ") {
            block.clear();
        }
        if line.contains("] === ON ") || line.contains("] header: ") {
            block.push_str(&line);
            block.push('\n');
        }
    }
    block
}

/// Deletes every report; returns how many went.
pub fn delete_reports(detailed: &Path) -> usize {
    let Ok(entries) = fs::read_dir(reports_dir(detailed)) else {
        return 0;
    };
    let count = entries
        .flatten()
        .filter(|entry| fs::remove_dir_all(entry.path()).is_ok())
        .count();
    let _ = fs::remove_dir(reports_dir(detailed));
    count
}
