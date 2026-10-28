//! "Download updates ahead": a newer release found by the update check is
//! downloaded and checked in the background, and kept next to the installed
//! exe as `wincraft.staged.exe`, with `wincraft.staged.json` saying which
//! release it is and the checksum it matched. Update now then takes it
//! instead of downloading, so the update takes a few seconds. Nothing is
//! ever installed from here without the user's Enter or click.
//!
//! Never on a metered connection, and a staged file goes away when a newer
//! one replaces it, after the update, and when the setting is switched off.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::NetworkManagement::IpHelper::GetNetworkConnectivityHint;
use windows_sys::Win32::Networking::WinSock::{
    NetworkConnectivityCostHintFixed, NetworkConnectivityCostHintVariable,
    NL_NETWORK_CONNECTIVITY_HINT,
};

use crate::core::sha256;
use crate::core::update::{self, Fetch, NEW_EXE};
use crate::store::github::{self, Release};

pub const STAGED_EXE: &str = "wincraft.staged.exe";
pub const RECORD: &str = "wincraft.staged.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    tag: String,
    sha256: String,
}

fn record(dir: &Path) -> Option<Record> {
    let text = fs::read_to_string(dir.join(RECORD)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Downloads `release` into `dir` and keeps it, after any file staged
/// before is deleted. Only a file that matches its checksum is kept.
pub fn stage(
    release: &Release,
    dir: &Path,
    fetch: &dyn Fetch,
    local_fake: bool,
) -> Result<(), String> {
    remove(dir);
    let (_, sha256) =
        update::download_as(release, dir, STAGED_EXE, fetch, local_fake, &mut |_, _| {
            true
        })?;
    let text = serde_json::to_string(&Record {
        tag: release.tag.clone(),
        sha256,
    })
    .map_err(|err| err.to_string())?;
    if let Err(err) = fs::write(dir.join(RECORD), text) {
        remove(dir);
        return Err(format!("cannot write {RECORD}: {err}"));
    }
    Ok(())
}

/// The release a file is staged for, when there is one. Cheap: it reads
/// the record and looks for the file, without hashing it.
pub fn staged_tag(dir: &Path) -> Option<String> {
    let record = record(dir)?;
    dir.join(STAGED_EXE).is_file().then_some(record.tag)
}

/// The staged file for `tag`, renamed to `wincraft.new.exe` for the swap,
/// when it still matches its checksum. A staged file for another release,
/// or one that changed on disk, is deleted and None comes back: Update now
/// then downloads as it always did.
pub fn take(dir: &Path, tag: &str) -> Option<PathBuf> {
    let record = record(dir)?;
    let staged = dir.join(STAGED_EXE);
    let matches =
        record.tag == tag && sha256::of_file(&staged).is_ok_and(|actual| actual == record.sha256);
    if !matches {
        log::info!(
            "the staged {} does not fit {tag}; it is deleted",
            record.tag
        );
        remove(dir);
        return None;
    }
    let new = dir.join(NEW_EXE);
    let _ = fs::remove_file(&new);
    let moved = fs::rename(&staged, &new);
    let _ = fs::remove_file(dir.join(RECORD));
    match moved {
        Ok(()) => Some(new),
        Err(err) => {
            log::warn!("could not use the staged {tag}: {err}");
            remove(dir);
            None
        }
    }
}

/// Deletes the staged file and its record, if there are any.
pub fn remove(dir: &Path) {
    for name in [STAGED_EXE, RECORD] {
        let _ = fs::remove_file(dir.join(name));
    }
}

/// Deletes a staged file that is no newer than `current`: it was installed,
/// or a newer version came another way.
pub fn tidy(dir: &Path, current: &str) {
    if let Some(tag) = record(dir).map(|record| record.tag) {
        if !github::is_newer(&tag, current) {
            log::info!("the staged {tag} is not newer than {current}; it is deleted");
            remove(dir);
        }
    }
}

/// Whether a newer release should be downloaded ahead now: the user wants
/// it, this copy can update itself, it is not staged already and no
/// download runs. The metered test comes last, on the download's thread.
pub fn wanted(ahead: bool, can_update: bool, busy: bool, staged: Option<&str>, tag: &str) -> bool {
    ahead && can_update && !busy && staged != Some(tag)
}

/// A connection the user may pay for by the byte: Windows says its cost is
/// fixed or variable, or the plan is near or over its limit, or roaming.
pub fn is_metered(hint: &NL_NETWORK_CONNECTIVITY_HINT) -> bool {
    hint.ConnectivityCost == NetworkConnectivityCostHintFixed
        || hint.ConnectivityCost == NetworkConnectivityCostHintVariable
        || hint.ApproachingDataLimit
        || hint.OverDataLimit
        || hint.Roaming
}

/// Whether the PC's connection is metered now; true when Windows cannot
/// say, since a download nobody asked for must not cost money.
pub fn metered_now() -> bool {
    let mut hint = NL_NETWORK_CONNECTIVITY_HINT::default();
    let status = unsafe { GetNetworkConnectivityHint(&mut hint) };
    if status != 0 {
        log::info!("Windows could not say whether the connection is metered ({status})");
        return true;
    }
    is_metered(&hint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::update::{EXE_ASSET, SUMS_ASSET};
    use crate::store::github::Asset;
    use std::collections::BTreeMap;
    use windows_sys::Win32::Networking::WinSock::{
        NetworkConnectivityCostHintUnknown, NetworkConnectivityCostHintUnrestricted,
    };

    const DOWNLOADS: &str = "https://github.com/OliverD25/wincraft/releases/download/";

    struct FakeFetch {
        files: BTreeMap<String, Vec<u8>>,
    }

    impl Fetch for FakeFetch {
        fn text(&self, url: &str) -> Result<String, String> {
            self.files
                .get(url)
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .ok_or_else(|| "http status: 404".to_string())
        }

        fn to_file(
            &self,
            url: &str,
            dest: &Path,
            _progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
        ) -> Result<(), String> {
            let bytes = self.files.get(url).ok_or("http status: 404")?;
            fs::write(dest, bytes).map_err(|e| e.to_string())
        }
    }

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.to_string(),
            assets: [EXE_ASSET, SUMS_ASSET]
                .iter()
                .map(|name| Asset {
                    name: name.to_string(),
                    url: format!("{DOWNLOADS}{tag}/{name}"),
                })
                .collect(),
            ..Release::default()
        }
    }

    /// A release whose exe is `exe`, with the checksum file `sums` says.
    fn fetch(tag: &str, exe: &[u8], sums_of: &[u8]) -> FakeFetch {
        let sums = format!("{}  wincraft.exe\n", sha256::of_bytes(sums_of).unwrap());
        let mut files = BTreeMap::new();
        files.insert(format!("{DOWNLOADS}{tag}/{EXE_ASSET}"), exe.to_vec());
        files.insert(format!("{DOWNLOADS}{tag}/{SUMS_ASSET}"), sums.into_bytes());
        FakeFetch { files }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wincraft-stage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_staged_file_is_used_for_its_own_release_only() {
        let dir = scratch("own");
        stage(
            &release("v9.0.0"),
            &dir,
            &fetch("v9.0.0", b"nine", b"nine"),
            false,
        )
        .unwrap();
        assert_eq!(staged_tag(&dir).as_deref(), Some("v9.0.0"));
        assert_eq!(take(&dir, "v9.0.1"), None, "another release");
        assert!(!dir.join(STAGED_EXE).exists(), "the stale file is deleted");
        assert!(!dir.join(RECORD).exists());

        stage(
            &release("v9.0.0"),
            &dir,
            &fetch("v9.0.0", b"nine", b"nine"),
            false,
        )
        .unwrap();
        let new = take(&dir, "v9.0.0").expect("the staged file");
        assert_eq!(new, dir.join(NEW_EXE));
        assert_eq!(fs::read(&new).unwrap(), b"nine");
        assert!(!dir.join(STAGED_EXE).exists() && !dir.join(RECORD).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_staged_file_that_changed_on_disk_is_not_used() {
        let dir = scratch("changed");
        stage(
            &release("v9.0.0"),
            &dir,
            &fetch("v9.0.0", b"nine", b"nine"),
            false,
        )
        .unwrap();
        fs::write(dir.join(STAGED_EXE), b"tampered").unwrap();
        assert_eq!(take(&dir, "v9.0.0"), None);
        assert!(!dir.join(STAGED_EXE).exists());
        assert!(!dir.join(NEW_EXE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_download_that_does_not_match_is_not_staged() {
        let dir = scratch("bad");
        let err = stage(
            &release("v9.0.0"),
            &dir,
            &fetch("v9.0.0", b"evil", b"nine"),
            false,
        );
        assert!(err.is_err());
        assert_eq!(staged_tag(&dir), None);
        assert!(!dir.join(STAGED_EXE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_release_replaces_the_staged_one() {
        let dir = scratch("newer");
        stage(
            &release("v9.0.0"),
            &dir,
            &fetch("v9.0.0", b"nine", b"nine"),
            false,
        )
        .unwrap();
        stage(
            &release("v9.1.0"),
            &dir,
            &fetch("v9.1.0", b"ninety", b"ninety"),
            false,
        )
        .unwrap();
        assert_eq!(staged_tag(&dir).as_deref(), Some("v9.1.0"));
        assert_eq!(fs::read(dir.join(STAGED_EXE)).unwrap(), b"ninety");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tidy_deletes_what_is_not_newer_than_the_running_version() {
        let dir = scratch("tidy");
        stage(
            &release("v9.0.0"),
            &dir,
            &fetch("v9.0.0", b"nine", b"nine"),
            false,
        )
        .unwrap();
        tidy(&dir, "8.9.9");
        assert_eq!(staged_tag(&dir).as_deref(), Some("v9.0.0"), "still newer");
        tidy(&dir, "9.0.0");
        assert_eq!(staged_tag(&dir), None, "installed now");
        assert!(!dir.join(STAGED_EXE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_release_is_downloaded_ahead_only_when_wanted_and_not_there_yet() {
        assert!(wanted(true, true, false, None, "v2"));
        assert!(
            wanted(true, true, false, Some("v1"), "v2"),
            "an older one is replaced"
        );
        assert!(!wanted(false, true, false, None, "v2"), "switched off");
        assert!(
            !wanted(true, false, false, None, "v2"),
            "this copy cannot update"
        );
        assert!(!wanted(true, true, true, None, "v2"), "a download runs");
        assert!(
            !wanted(true, true, false, Some("v2"), "v2"),
            "already there"
        );
    }

    #[test]
    fn a_metered_connection_is_one_that_may_cost_money() {
        let hint = |cost| NL_NETWORK_CONNECTIVITY_HINT {
            ConnectivityCost: cost,
            ..Default::default()
        };
        assert!(!is_metered(&hint(NetworkConnectivityCostHintUnrestricted)));
        assert!(!is_metered(&hint(NetworkConnectivityCostHintUnknown)));
        assert!(is_metered(&hint(NetworkConnectivityCostHintFixed)));
        assert!(is_metered(&hint(NetworkConnectivityCostHintVariable)));
        for flag in 0..3 {
            let mut near = hint(NetworkConnectivityCostHintUnrestricted);
            match flag {
                0 => near.ApproachingDataLimit = true,
                1 => near.OverDataLimit = true,
                _ => near.Roaming = true,
            }
            assert!(is_metered(&near), "{flag}");
        }
    }
}
