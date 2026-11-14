//! Editing a made-up Claude Code settings.json. The user's real file is
//! never read or written: every file test uses its own scratch folder.

use super::*;

const EXE: &str = r"C:\Users\Someone\AppData\Local\Programs\WinCraft\wincraft.exe";

/// A settings file as Claude Code writes it, with keys out of alphabetical
/// order and a hook of the user's own.
const ORIGINAL: &str = r#"{
  "model": "opus",
  "env": {
    "Z_LAST": "1",
    "A_FIRST": "2"
  },
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python my-stop.py"
          }
        ]
      }
    ]
  },
  "alwaysThinkingEnabled": true
}
"#;

fn command() -> String {
    command_for(Path::new(EXE))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wincraft-hook-settings-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn the_command_names_the_exe_with_forward_slashes() {
    assert_eq!(
        command(),
        "\"C:/Users/Someone/AppData/Local/Programs/WinCraft/wincraft.exe\" --claude-hook"
    );
    assert!(is_ours(&command()));
    assert!(is_ours(r#""D:\Tools\WinCraft.exe" --claude-hook"#));
    assert!(!is_ours("python claude-attention.py"));
    assert!(!is_ours(r#""wincraft.exe" --waiting x"#));
}

#[test]
fn connect_adds_one_group_per_event_and_keeps_everything_else() {
    let text = connect(Some(ORIGINAL), &command()).unwrap().unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    for event in EVENTS {
        let groups = value["hooks"][event].as_array().unwrap();
        let ours: Vec<&Value> = groups
            .iter()
            .filter(|g| g["hooks"][0]["command"] == command().as_str())
            .collect();
        assert_eq!(ours.len(), 1, "{event}");
        assert_eq!(ours[0]["hooks"][0]["timeout"], 5);
        assert_eq!(ours[0]["hooks"][0]["type"], "command");
        let matcher = ours[0].get("matcher").and_then(Value::as_str);
        let wanted = (event == "PreToolUse").then_some("AskUserQuestion");
        assert_eq!(matcher, wanted, "{event}");
    }
    // The user's own Stop hook stays first.
    assert_eq!(
        value["hooks"]["Stop"][0]["hooks"][0]["command"],
        "python my-stop.py"
    );
    assert_eq!(value["model"], "opus");
    assert_eq!(value["alwaysThinkingEnabled"], true);
    // The keys keep their order.
    let at = |key: &str| text.find(key).unwrap();
    assert!(at("\"model\"") < at("\"env\""));
    assert!(at("\"Z_LAST\"") < at("\"A_FIRST\""));
    assert!(at("\"hooks\"") < at("\"alwaysThinkingEnabled\""));
    assert!(text.ends_with("}\n"));
}

#[test]
fn connecting_twice_adds_nothing() {
    let once = connect(Some(ORIGINAL), &command()).unwrap().unwrap();
    assert_eq!(connect(Some(&once), &command()).unwrap(), None);
}

#[test]
fn connect_points_an_old_entry_at_the_exe_running_now() {
    let once = connect(Some(ORIGINAL), r#""C:/Old/wincraft.exe" --claude-hook"#)
        .unwrap()
        .unwrap();
    let again = connect(Some(&once), &command()).unwrap().unwrap();
    assert!(!again.contains("C:/Old/"));
    assert_eq!(again.matches("--claude-hook").count(), EVENTS.len());
}

#[test]
fn disconnect_gives_back_the_file_it_was_connected_from() {
    let connected = connect(Some(ORIGINAL), &command()).unwrap().unwrap();
    assert_eq!(disconnect(&connected).unwrap().as_deref(), Some(ORIGINAL));
    assert_eq!(disconnect(ORIGINAL).unwrap(), None);
    // A file with only WinCraft's hook loses its "hooks" key.
    let alone = connect(Some("{\n  \"model\": \"opus\"\n}"), &command())
        .unwrap()
        .unwrap();
    assert_eq!(
        disconnect(&alone).unwrap().as_deref(),
        Some("{\n  \"model\": \"opus\"\n}")
    );
}

#[test]
fn disconnect_keeps_other_hooks_in_a_shared_group() {
    let shared = format!(
        r#"{{"hooks":{{"Stop":[{{"hooks":[{{"type":"command","command":"x.py"}},{{"type":"command","command":{}}}]}}]}}}}"#,
        serde_json::to_string(&command()).unwrap()
    );
    let text = disconnect(&shared).unwrap().unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        value["hooks"]["Stop"][0]["hooks"].as_array().unwrap().len(),
        1
    );
    assert_eq!(value["hooks"]["Stop"][0]["hooks"][0]["command"], "x.py");
}

#[test]
fn a_file_that_is_not_settings_is_never_changed() {
    assert!(connect(Some("{ not json"), &command()).is_err());
    assert!(connect(Some("[1, 2]"), &command()).is_err());
    assert!(connect(Some(r#"{"hooks": 3}"#), &command()).is_err());
    assert!(connect(Some(r#"{"hooks": {"Stop": {}}}"#), &command()).is_err());
    assert!(disconnect("{ not json").is_err());
}

#[test]
fn the_status_line() {
    assert_eq!(status(None), Status::NotConnected);
    assert_eq!(status(Some(ORIGINAL)), Status::NotConnected);
    assert_eq!(status(Some("{ broken")), Status::Unreadable);
    let connected = connect(Some(ORIGINAL), &command()).unwrap().unwrap();
    assert_eq!(status(Some(&connected)), Status::Connected);
    assert!(Status::Connected.connected());
    let other = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"python \"C:/x/claude-attention.py\""}]}]}}"#;
    assert_eq!(status(Some(other)), Status::Other);
    assert!(!Status::Other.connected());
    let both = connect(Some(other), &command()).unwrap().unwrap();
    assert_eq!(status(Some(&both)), Status::ConnectedAndOther);
    // One event missing is not connected.
    let partial = connected.replacen("\"UserPromptSubmit\"", "\"SomethingElse\"", 1);
    assert_eq!(status(Some(&partial)), Status::NotConnected);
    assert_eq!(Status::Connected.line(), "Connected to Claude Code");
    assert_eq!(Status::NotConnected.line(), "Not connected");
    assert_eq!(
        Status::Other.line(),
        "Another hook already marks chats (claude-attention.py)"
    );
}

#[test]
fn files_are_created_backed_up_once_and_restored() {
    let dir = scratch("files");
    let path = dir.join(".claude").join("settings.json");
    let exe = Path::new(EXE);
    // No file and no folder yet: both are made, with no backup.
    assert_eq!(connect_file(&path, exe), Ok(true));
    assert!(!backup_path(&path).exists());
    assert_eq!(status_of(&path), Status::Connected);
    assert_eq!(connect_file(&path, exe), Ok(false));
    assert_eq!(disconnect_file(&path), Ok(true));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}\n");

    // The user's own file: backed up before the first change.
    let path = dir.join("other").join("settings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, ORIGINAL).unwrap();
    assert_eq!(connect_file(&path, exe), Ok(true));
    assert_eq!(
        std::fs::read_to_string(backup_path(&path)).unwrap(),
        ORIGINAL
    );
    assert_eq!(disconnect_file(&path), Ok(true));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), ORIGINAL);
    // The first backup is kept.
    std::fs::write(&path, "{\"x\": 1}\n").unwrap();
    assert_eq!(connect_file(&path, exe), Ok(true));
    assert_eq!(
        std::fs::read_to_string(backup_path(&path)).unwrap(),
        ORIGINAL
    );

    std::fs::write(&path, "{ broken").unwrap();
    assert!(connect_file(&path, exe).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ broken");
    assert_eq!(disconnect_file(&dir.join("none.json")), Ok(false));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_balloons() {
    let path = Path::new(r"C:\x\settings.json");
    assert_eq!(
        balloon(true, &Ok(true), path),
        "WinCraft connected to Claude Code. New chats show when they ask you."
    );
    assert_eq!(
        balloon(false, &Ok(false), path),
        "WinCraft disconnected from Claude Code."
    );
    let error = balloon(true, &Err("it is not valid JSON".into()), path);
    assert!(error.contains(r"C:\x\settings.json") && error.contains("not valid JSON"));
}
