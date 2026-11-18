//! The transcripts on disk and what was read of each: the byte offset, the
//! size and time seen, and the tokens found so far, in buckets by local
//! day, hour and model. A pass reads only the bytes after the offset; a
//! file that shrank is read again from the start, and its old buckets go.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use super::parse::{self, Reply, Tokens};

/// A local day (days since 1970-01-01 in local time) and hour.
pub type Slot = (i32, u8);

/// Tokens of one model in one local hour.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bucket {
    pub day: i32,
    pub hour: u8,
    pub model: String,
    pub tokens: Tokens,
}

/// What was read of one transcript.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileState {
    /// The chat's Claude Code session id; a subagent's file counts for
    /// the chat that started it.
    pub session: String,
    /// The project folder's name under `projects`.
    pub folder: String,
    pub subagent: bool,
    pub offset: u64,
    pub size: u64,
    pub modified_ms: u64,
    pub buckets: Vec<Bucket>,
    /// The id of the last reply counted, with where it went and what it
    /// held: the next line with the same id replaces it.
    pub last_id: Option<String>,
    pub last_bucket: Option<usize>,
    pub last_tokens: Tokens,
    /// The last reply's time and context, for "context now".
    pub context: Option<(u64, u64)>,
}

impl FileState {
    /// Counts one reply. A reply split over several lines is counted once.
    pub fn count(&mut self, reply: &Reply, slot: Slot) {
        if reply.id.is_some() && reply.id == self.last_id {
            if let Some(bucket) = self.last_bucket.and_then(|at| self.buckets.get_mut(at)) {
                bucket.tokens.take(&self.last_tokens);
            }
        }
        let at = match self
            .buckets
            .iter()
            .position(|b| (b.day, b.hour) == slot && b.model == reply.model)
        {
            Some(at) => at,
            None => {
                self.buckets.push(Bucket {
                    day: slot.0,
                    hour: slot.1,
                    model: reply.model.clone(),
                    tokens: Tokens::default(),
                });
                self.buckets.len() - 1
            }
        };
        self.buckets[at].tokens.add(&reply.tokens);
        self.last_id = reply.id.clone();
        self.last_bucket = Some(at);
        self.last_tokens = reply.tokens;
        if self.context.is_none_or(|(when, _)| reply.at_ms >= when) {
            self.context = Some((reply.at_ms, reply.tokens.context()));
        }
    }

    /// Forgets what was read, to read the file again from the start.
    fn restart(&mut self) {
        *self = FileState {
            session: std::mem::take(&mut self.session),
            folder: std::mem::take(&mut self.folder),
            subagent: self.subagent,
            ..FileState::default()
        };
    }

    /// Drops the buckets of days before `first_day`.
    pub fn drop_before(&mut self, first_day: i32) {
        let before = self.buckets.len();
        self.buckets.retain(|b| b.day >= first_day);
        if self.buckets.len() != before {
            self.last_id = None;
            self.last_bucket = None;
        }
    }
}

/// A transcript found on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub session: String,
    pub folder: String,
    pub subagent: bool,
    pub size: u64,
    pub modified_ms: u64,
}

fn modified_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

fn found(path: PathBuf, session: &str, folder: &str, subagent: bool) -> Option<Found> {
    let meta = std::fs::metadata(&path).ok()?;
    Some(Found {
        session: session.to_string(),
        folder: folder.to_string(),
        subagent,
        size: meta.len(),
        modified_ms: modified_ms(&meta),
        path,
    })
}

fn is_jsonl(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
}

/// Every `.jsonl` under a chat's `subagents` folder, at any depth.
fn subagent_files(dir: &Path, session: &str, folder: &str, into: &mut Vec<Found>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subagent_files(&path, session, folder, into);
        } else if is_jsonl(&path) {
            into.extend(found(path, session, folder, true));
        }
    }
}

/// The transcripts under `projects` changed since `since_ms`: each chat's
/// `<folder>\<session>.jsonl` and its subagents'
/// `<folder>\<session>\subagents\*.jsonl`.
pub fn list(projects: &Path, since_ms: u64) -> Vec<Found> {
    let mut all = Vec::new();
    let Ok(folders) = std::fs::read_dir(projects) else {
        return all;
    };
    for folder in folders.flatten() {
        let folder_path = folder.path();
        if !folder_path.is_dir() {
            continue;
        }
        let name = folder.file_name().to_string_lossy().into_owned();
        let Ok(entries) = std::fs::read_dir(&folder_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_dir() {
                subagent_files(&path.join("subagents"), &stem, &name, &mut all);
            } else if is_jsonl(&path) {
                all.extend(found(path, &stem, &name, false));
            }
        }
    }
    all.retain(|file| file.modified_ms >= since_ms);
    all
}

/// Whether a file changed since it was last read.
pub fn changed(state: Option<&FileState>, file: &Found) -> bool {
    state.is_none_or(|s| s.size != file.size || s.modified_ms != file.modified_ms)
}

/// Reads the lines added to a file since its offset, and counts their
/// replies. `slot` turns a reply's time into its local day and hour. A
/// line not ended yet is left for the next pass.
pub fn read_new(
    path: &Path,
    state: &mut FileState,
    slot: &dyn Fn(u64) -> Slot,
) -> std::io::Result<()> {
    let mut file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    let size = meta.len();
    if size < state.offset {
        state.restart();
    }
    file.seek(SeekFrom::Start(state.offset))?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 || line.last() != Some(&b'\n') {
            break;
        }
        state.offset += read as u64;
        if let Some(reply) = std::str::from_utf8(&line).ok().and_then(parse::reply) {
            state.count(&reply, slot(reply.at_ms));
        }
    }
    state.size = size;
    state.modified_ms = modified_ms(&meta);
    Ok(())
}

/// Everything that was read, by path. Saved so a restart does not read
/// the transcripts again.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Book {
    pub version: u32,
    pub files: BTreeMap<String, FileState>,
}

pub const BOOK_VERSION: u32 = 1;
