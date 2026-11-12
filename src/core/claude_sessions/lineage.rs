//! Where a chat runs and which chat it came from: its folder, worktree,
//! branch and model, and the chat it was forked from or started by. Read
//! from the same session file; every field may be missing.

use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lineage {
    pub cwd: Option<String>,
    /// The folder the chat was started in; for a chat in a worktree, the
    /// repository the worktree belongs to.
    pub origin_cwd: Option<String>,
    pub worktree_name: Option<String>,
    pub branch: Option<String>,
    pub model: Option<String>,
    /// The `sessionId` of the chat this one was forked from.
    pub forked_from: Option<String>,
    /// The `sessionId` of the chat that started this one as a side chat.
    pub spawned_from: Option<String>,
    /// Unix milliseconds.
    pub created: Option<u64>,
    pub last_focused: Option<u64>,
    pub interrupted: bool,
}

impl Lineage {
    /// The chat this one hangs under: the fork's source, else the chat that
    /// started it.
    pub fn parent(&self) -> Option<&str> {
        self.forked_from.as_deref().or(self.spawned_from.as_deref())
    }

    /// The project folder: where the chat started, else where it runs.
    pub fn folder(&self) -> Option<&str> {
        self.origin_cwd.as_deref().or(self.cwd.as_deref())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpawnedFrom {
    #[serde(default)]
    pub session_id: Option<String>,
}

/// `spawnedFrom` from a JSON value of any shape.
pub(super) fn spawned_from(value: &serde_json::Value) -> Option<SpawnedFrom> {
    let id = value.get("sessionId")?.as_str()?;
    Some(SpawnedFrom {
        session_id: Some(id.to_string()),
    })
}

/// Trimmed text, or None for none.
pub(super) fn text(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}
