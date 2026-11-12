use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| arg.to_string()).collect()
}

const CLAUDE: &str =
    r"C:\Program Files\WindowsApps\Claude_4.16336.0.0_x64__pzs8sxrjxfjjc\app\claude.exe";
const CHROME: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";

#[test]
fn the_flags_take_the_next_argument_as_the_title() {
    assert_eq!(
        parse_args(&args(&["wincraft.exe", "--waiting", "Fix the build"])),
        Some(Ok((Kind::Waiting, "Fix the build".to_string())))
    );
    assert_eq!(
        parse_args(&args(&["wincraft.exe", "--answered", "  Chat  "])),
        Some(Ok((Kind::Answered, "Chat".to_string())))
    );
    assert_eq!(parse_args(&args(&["wincraft.exe"])), None);
    assert_eq!(parse_args(&args(&["wincraft.exe", "--quit"])), None);
    // The program name itself is never taken for a flag.
    assert_eq!(parse_args(&args(&["--waiting", "x"])), None);
    // Exact flags only.
    assert_eq!(parse_args(&args(&["wincraft.exe", "--WAITING", "x"])), None);
}

#[test]
fn a_missing_or_empty_title_is_an_error_line() {
    for list in [
        &["wincraft.exe", "--waiting"][..],
        &["wincraft.exe", "--answered", "   "][..],
    ] {
        match parse_args(&args(list)) {
            Some(Err(line)) => assert!(line.contains("needs the session title"), "{line}"),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_copy_round_trips_and_strangers_are_refused() {
    let units = encode(" Fix the build ");
    let bytes: Vec<u8> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    assert_eq!(
        decode(Kind::Waiting.code(), &bytes),
        Some((Kind::Waiting, "Fix the build".to_string()))
    );
    assert_eq!(
        decode(Kind::Answered.code(), &bytes),
        Some((Kind::Answered, "Fix the build".to_string()))
    );
    assert_eq!(decode(1, &bytes), None);
    assert_eq!(decode(Kind::Waiting.code(), &bytes[1..]), None);
    assert_eq!(decode(Kind::Waiting.code(), &[]), None);
    assert_eq!(encode(&"x".repeat(2000)).len(), 512);
    let unicode = encode("Перевірка ✓");
    let bytes: Vec<u8> = unicode.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    assert_eq!(
        decode(Kind::Waiting.code(), &bytes).unwrap().1,
        "Перевірка ✓"
    );
}

#[test]
fn only_a_claude_window_with_exactly_that_title_matches() {
    assert!(shows_session(CLAUDE, "Fix the build", "Fix the build"));
    assert!(shows_session(CLAUDE, " Fix the build ", "Fix the build  "));
    assert!(!shows_session(CLAUDE, "Fix the build 2", "Fix the build"));
    assert!(!shows_session(CLAUDE, "fix the build", "Fix the build"));
    assert!(!shows_session(CHROME, "Fix the build", "Fix the build"));
    assert!(!shows_session(CLAUDE, "", ""));
    assert!(is_claude_app(
        r"windowsapps\claude_pzs8sxrjxfjjc\app\claude.exe"
    ));
    assert!(!is_claude_app(
        r"windowsapps\microsoft.windowsterminal_8wekyb3d8bbwe\windowsterminal.exe"
    ));
}

#[test]
fn the_reply_counts_windows_and_zero_means_not_taken() {
    assert_eq!(reply_for(0), 1);
    assert_eq!(reply_for(2), 3);
    assert_eq!(
        Outcome::Taken {
            kind: Kind::Waiting,
            matched: 2
        }
        .message("Chat"),
        "WinCraft marked 2 Claude windows titled \"Chat\""
    );
    assert_eq!(
        Outcome::Taken {
            kind: Kind::Answered,
            matched: 1
        }
        .message("Chat"),
        "WinCraft cleared the mark of 1 Claude window titled \"Chat\""
    );
    assert_eq!(Outcome::NotRunning.exit_code(), 1);
    assert_eq!(Outcome::NoAnswer.exit_code(), 1);
    assert_eq!(Outcome::BadArgs(String::new()).exit_code(), 2);
    assert_eq!(
        Outcome::Taken {
            kind: Kind::Waiting,
            matched: 0
        }
        .exit_code(),
        0
    );
}

#[test]
fn a_window_can_be_named_by_its_handle() {
    assert_eq!(window_handle("hwnd:265740"), Some(265740));
    assert_eq!(window_handle(" hwnd:0x40e0c "), Some(0x40e0c));
    assert_eq!(window_handle("hwnd:0"), None);
    assert_eq!(window_handle("hwnd:abc"), None);
    assert_eq!(window_handle("WinCraft Rust"), None);
}

#[test]
fn the_kind_is_optional_and_only_approval_or_question() {
    assert_eq!(
        parse_ask(&args(&["wincraft.exe", "--waiting", "Chat"])),
        Ok(Ask::Question)
    );
    assert_eq!(
        parse_ask(&args(&[
            "wincraft.exe",
            "--waiting",
            "Chat",
            "--kind",
            "approval"
        ])),
        Ok(Ask::Approval)
    );
    assert_eq!(
        parse_ask(&args(&[
            "wincraft.exe",
            "--kind",
            "Question",
            "--waiting",
            "Chat"
        ])),
        Ok(Ask::Question)
    );
    assert!(parse_ask(&args(&[
        "wincraft.exe",
        "--waiting",
        "Chat",
        "--kind",
        "later"
    ]))
    .is_err());
    assert!(parse_ask(&args(&["wincraft.exe", "--waiting", "Chat", "--kind"])).is_err());
    // The title is still the word after --waiting.
    assert_eq!(
        parse_args(&args(&[
            "wincraft.exe",
            "--waiting",
            "Chat",
            "--kind",
            "approval"
        ])),
        Some(Ok((Kind::Waiting, "Chat".to_string())))
    );
}

#[test]
fn an_approval_copy_is_a_waiting_copy_with_its_own_code() {
    let bytes: Vec<u8> = encode("Chat")
        .iter()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    assert_eq!(code_for(Kind::Waiting, Ask::Question), Kind::Waiting.code());
    assert_eq!(
        code_for(Kind::Answered, Ask::Approval),
        Kind::Answered.code()
    );
    let code = code_for(Kind::Waiting, Ask::Approval);
    assert_eq!(
        decode_ask(code, &bytes),
        Some((Kind::Waiting, Ask::Approval, "Chat".to_string()))
    );
    assert_eq!(
        decode_ask(Kind::Waiting.code(), &bytes),
        Some((Kind::Waiting, Ask::Question, "Chat".to_string()))
    );
    // The old decoder does not take the new code.
    assert_eq!(decode(code, &bytes), None);
    assert_eq!(decode_ask(0x1234, &bytes), None);
}
