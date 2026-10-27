use super::*;
use crate::search::test_context;

fn hit(name: &str, path: &str, folder: bool, modified: u64) -> Hit {
    Hit {
        name: name.to_string(),
        path: path.to_string(),
        folder,
        drive: false,
        size: None,
        modified: Some(modified),
    }
}

fn answer(hits: Vec<Hit>) -> Answer {
    Answer {
        total: hits.len() as u32,
        hits,
    }
}

fn names<'a>(list: &[Ranked<'a>]) -> Vec<&'a str> {
    list.iter().map(|ranked| ranked.hit.name.as_str()).collect()
}

#[test]
fn an_exact_name_beats_a_prefix_beats_a_word_beats_a_fragment() {
    let exact = hit("pswds", r"D:\a", true, 0);
    let exact_file = hit("pswds.txt", r"D:\a", false, 0);
    let prefix = hit("pswds_OneDrive", r"D:\a", true, 0);
    let word = hit("old pswds", r"D:\a", true, 0);
    let fragment = hit("mypswdsold", r"D:\a", true, 0);
    let path_only = hit("misc", r"D:\pswds\misc", true, 0);
    let all = [&exact, &exact_file, &prefix, &word, &fragment, &path_only];
    let scores: Vec<i32> = all.iter().map(|h| match_score("pswds", h)).collect();
    assert!(scores[0] > scores[2], "{scores:?}");
    assert!(
        scores[1] > scores[2],
        "an extension does not spoil an exact name"
    );
    assert!(scores[2] > scores[3], "{scores:?}");
    assert!(scores[3] > scores[4], "{scores:?}");
    assert!(scores[4] > scores[5], "{scores:?}");
}

#[test]
fn a_shorter_name_beats_a_longer_one_of_the_same_kind() {
    let short = hit("pswds_a", r"D:\a", true, 0);
    let long = hit("pswds_a_very_long_folder_name_indeed", r"D:\a", true, 0);
    assert!(match_score("pswds", &short) > match_score("pswds", &long));
}

#[test]
fn a_file_never_scores_like_an_exact_app_name() {
    let exact = hit("chrome.exe", r"C:\x", false, 0);
    assert!(match_score("chrome", &exact) < 60);
}

#[test]
fn folders_come_first_and_files_score_below_them() {
    // The file is the better match, but the folder still leads.
    let reply = answer(vec![
        hit("report.docx", r"D:\a", false, 5),
        hit("old report archive", r"D:\a", true, 5),
    ]);
    let (folders, files) = blended("report", &reply, 6);
    assert_eq!(names(&folders), ["old report archive"]);
    assert_eq!(names(&files), ["report.docx"]);
    assert!(files[0].score < folders[0].score);
}

#[test]
fn folders_cannot_take_the_rows_files_are_kept() {
    let mut hits: Vec<Hit> = (0..9)
        .map(|n| hit(&format!("proj{n}"), r"D:\a", true, n))
        .collect();
    hits.push(hit("proj.txt", r"D:\a", false, 1));
    hits.push(hit("proj2.txt", r"D:\a", false, 2));
    hits.push(hit("proj3.txt", r"D:\a", false, 3));
    let reply = answer(hits);
    let (folders, files) = blended("proj", &reply, 6);
    assert_eq!(folders.len(), 4);
    assert_eq!(files.len(), 2);
    // With no files all six rows are folders.
    let only: Vec<Hit> = (0..9)
        .map(|n| hit(&format!("proj{n}"), r"D:\a", true, n))
        .collect();
    let reply = answer(only);
    let (folders, files) = blended("proj", &reply, 6);
    assert_eq!((folders.len(), files.len()), (6, 0));
}

#[test]
fn equal_matches_put_the_newest_first() {
    let reply = answer(vec![
        hit("proj1", r"D:\a", true, 10),
        hit("proj2", r"D:\a", true, 30),
        hit("proj3", r"D:\a", true, 20),
    ]);
    let (folders, _) = blended("proj", &reply, 6);
    assert_eq!(names(&folders), ["proj2", "proj3", "proj1"]);
}

#[test]
fn the_recycle_bin_and_recent_shortcuts_are_left_out() {
    let reply = answer(vec![
        hit("pswds", r"D:\$RECYCLE.BIN\S-1-5-21", true, 1),
        hit(
            "pswds.lnk",
            r"C:\Users\Admin\AppData\Roaming\Microsoft\Windows\Recent",
            false,
            1,
        ),
        hit("pswds", r"D:\Real", true, 1),
    ]);
    let (folders, files) = blended("pswds", &reply, 6);
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].hit.path, r"D:\Real");
    assert!(files.is_empty());
}

#[test]
fn a_folder_row_opens_shows_copies_and_browses() {
    let folder = hit("pswds_OneDrive", r"D:\Windows_OneDrive\OneDrive", true, 1);
    let item = row(
        Ranked {
            score: 40,
            hit: &folder,
        },
        "Folders",
        None,
    );
    let path = r"D:\Windows_OneDrive\OneDrive\pswds_OneDrive";
    assert_eq!(item.title, "pswds_OneDrive");
    assert_eq!(item.subtitle, r"D:\Windows_OneDrive\OneDrive");
    assert_eq!(item.usage_key.as_deref(), Some(&*path.to_lowercase()));
    assert_eq!(item.icon, IconRef::Path(PathBuf::from(path)));
    assert_eq!(
        item.enter.as_ref().map(|c| &c.action),
        Some(&Action::Open(PathBuf::from(path)))
    );
    assert_eq!(
        item.shift_enter.as_ref().map(|c| &c.action),
        Some(&Action::Reveal(PathBuf::from(path)))
    );
    assert_eq!(
        item.ctrl_enter.as_ref().map(|c| &c.action),
        Some(&Action::Copy(path.to_string()))
    );
    let tab = item.tab_action.expect("a folder can be browsed");
    assert_eq!(
        tab.action,
        Action::Provider {
            provider: "files".to_string(),
            command: format!("browse {path}"),
        }
    );
    let mut files = Files::new();
    let Action::Provider { command, .. } = tab.action else {
        unreachable!()
    };
    assert_eq!(
        files.act(&command, &test_context()),
        Reply::Switch {
            provider: "paths",
            text: format!("{path}\\"),
        }
    );
}

#[test]
fn a_file_row_has_no_browse() {
    let file = hit("notes.txt", r"D:\a", false, 1);
    let item = row(
        Ranked {
            score: 1,
            hit: &file,
        },
        "Files",
        None,
    );
    assert!(item.tab_action.is_none());
    assert!(item.tab.is_none());
}

#[test]
fn a_drive_row_says_so_and_ends_with_a_backslash() {
    let mut drive = hit("D:", "", true, 0);
    drive.drive = true;
    let item = row(
        Ranked {
            score: 1,
            hit: &drive,
        },
        "Folders",
        None,
    );
    assert_eq!(item.subtitle, "Drive");
    assert_eq!(
        item.enter.map(|c| c.action),
        Some(Action::Open(PathBuf::from(r"D:\")))
    );
}

fn failure_of(client: &Client, text: &str) -> Option<Status> {
    client
        .failed
        .as_ref()
        .filter(|(failed, _)| failed == text)
        .map(|(_, status)| *status)
}

fn reply_for(id: u32, name: &str) -> Incoming {
    let reply = answer(vec![hit(name, r"D:\a", true, 1)]);
    // Written back out in Everything's layout, and read by the real parser.
    Incoming::Reply(id, encode(&reply))
}

/// An answer in the layout of `EVERYTHING_IPC_LIST2`, for the client.
fn encode(reply: &Answer) -> Vec<u8> {
    let field = |text: &str| {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
        units
            .iter()
            .for_each(|unit| bytes.extend_from_slice(&unit.to_le_bytes()));
        bytes.extend_from_slice(&[0, 0]);
        bytes
    };
    let mut bytes = Vec::new();
    for value in [reply.total, reply.hits.len() as u32, 0, REQUEST_FIELDS, 1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let table = bytes.len();
    bytes.resize(table + reply.hits.len() * ipc::ITEM2_SIZE, 0);
    for (index, hit) in reply.hits.iter().enumerate() {
        let data_at = bytes.len() as u32;
        let at = table + index * ipc::ITEM2_SIZE;
        let flags = if hit.folder { ipc::ITEM_FOLDER } else { 0 };
        bytes[at..at + 4].copy_from_slice(&flags.to_le_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&data_at.to_le_bytes());
        bytes.extend(field(&hit.name));
        bytes.extend(field(&hit.path));
        bytes.extend_from_slice(&u64::MAX.to_le_bytes());
        bytes.extend_from_slice(&hit.modified.unwrap_or(0).to_le_bytes());
    }
    bytes
}

#[test]
fn a_search_is_sent_once_and_shown_when_its_reply_comes() {
    let mut client = Client::default();
    let now = Instant::now();
    let Next::Send(id) = client.ask("pswds", now) else {
        panic!("a new search is sent");
    };
    assert!(client.waiting());
    assert_eq!(client.ask("pswds", now), Next::Wait, "not sent twice");
    assert!(client.answer("pswds").is_none());
    client.receive(reply_for(id, "pswds_OneDrive"));
    assert!(!client.waiting());
    assert!(client.take_news());
    assert!(!client.take_news(), "news is told once");
    assert_eq!(client.ask("pswds", now), Next::Show);
    assert_eq!(
        client.answer("pswds").unwrap().hits[0].name,
        "pswds_OneDrive"
    );
}

#[test]
fn a_newer_search_supersedes_an_older_reply() {
    let mut client = Client::default();
    let now = Instant::now();
    let Next::Send(first) = client.ask("psw", now) else {
        panic!()
    };
    let Next::Send(second) = client.ask("pswd", now) else {
        panic!()
    };
    assert_ne!(first, second);
    // The old reply arrives late: it must not show under the new text.
    client.receive(reply_for(first, "psw_old"));
    assert!(!client.take_news());
    assert!(client.answer("psw").is_none());
    assert!(client.answer("pswd").is_none());
    assert!(client.waiting());
    client.receive(reply_for(second, "pswd_new"));
    assert!(client.take_news());
    assert_eq!(client.answer("pswd").unwrap().hits[0].name, "pswd_new");
}

#[test]
fn a_reply_that_does_not_come_is_given_up() {
    let mut client = Client::default();
    let now = Instant::now();
    assert!(matches!(client.ask("pswds", now), Next::Send(_)));
    client.expire(now + GIVE_UP - Duration::from_millis(1));
    assert!(client.waiting());
    client.expire(now + GIVE_UP);
    assert!(!client.waiting());
    assert!(client.take_news());
    assert_eq!(failure_of(&client, "pswds"), Some(Status::NotAnswering));
    // The same text is not sent again in a loop; other text is.
    assert_eq!(client.ask("pswds", now + GIVE_UP), Next::Show);
    assert!(matches!(client.ask("pswd", now + GIVE_UP), Next::Send(_)));
}

#[test]
fn a_refusal_from_the_worker_becomes_a_status() {
    let mut client = Client::default();
    let now = Instant::now();
    let Next::Send(id) = client.ask("pswds", now) else {
        panic!()
    };
    client.receive(Incoming::Failed(id, Status::Loading));
    assert!(client.take_news());
    assert_eq!(failure_of(&client, "pswds"), Some(Status::Loading));
    assert!(client.answer("pswds").is_none());
    // A later good answer for other text clears it.
    let Next::Send(id) = client.ask("pswd", now) else {
        panic!()
    };
    client.receive(reply_for(id, "pswd"));
    assert_eq!(failure_of(&client, "pswd"), None);
}

#[test]
fn an_unreadable_reply_is_a_refusal_not_a_crash() {
    let mut client = Client::default();
    let Next::Send(id) = client.ask("pswds", Instant::now()) else {
        panic!()
    };
    client.receive(Incoming::Reply(id, vec![1, 2, 3]));
    assert_eq!(failure_of(&client, "pswds"), Some(Status::Refused));
}

#[test]
fn ids_are_never_zero_even_after_wrapping() {
    let mut client = Client {
        last_id: u32::MAX,
        ..Client::default()
    };
    assert_eq!(client.ask("ab", Instant::now()), Next::Send(1));
}

#[test]
fn a_short_search_asks_nobody() {
    let mut files = Files::new();
    let query = Query {
        text: "p".to_string(),
        prefix: None,
        limit: 8,
    };
    assert!(files.query(&query, &test_context()).is_empty());
    assert!(!files.waiting());
}

fn query_for(text: &str, prefixed: bool) -> Query {
    Query {
        text: text.to_string(),
        prefix: prefixed.then(|| ".".to_string()),
        limit: if prefixed { 500 } else { 8 },
    }
}

/// A provider that has been answered for `text`, as after the reply came.
fn answered(text: &str, hits: Vec<Hit>) -> Files {
    let mut files = Files::new();
    let now = Instant::now();
    let Next::Send(id) = files.client.ask(text, now) else {
        panic!()
    };
    files
        .client
        .receive(Incoming::Reply(id, encode(&answer(hits))));
    files
}

#[test]
fn the_dot_prefix_lists_folders_and_files_in_one_group_up_to_fifty() {
    let mut hits: Vec<Hit> = (0..70)
        .map(|n| hit(&format!("proj{n:02}.txt"), r"D:\a", false, n))
        .collect();
    hits.push(hit("proj", r"D:\a", true, 1));
    let mut files = answered("proj", hits);
    let (rows, job) = files.rows(&query_for("proj", true), Source::Everything, Instant::now());
    assert!(job.is_none(), "the answer is in hand");
    assert_eq!(rows.len(), 50);
    assert!(rows.iter().all(|item| item.group == "Files and folders"));
    assert_eq!(rows[0].title, "proj", "the exact folder name leads");
    // Without the prefix the list is the short one.
    let (rows, _) = files.rows(
        &query_for("proj", false),
        Source::Everything,
        Instant::now(),
    );
    assert_eq!(rows.len(), 6);
}

#[test]
fn a_folder_leads_a_file_that_matches_as_well() {
    let files = [
        hit("notes.txt", r"D:\a", false, 99),
        hit("notes.txt", r"D:\b", true, 1),
    ];
    let list = ranked("notes", files.iter());
    assert!(list[0].hit.folder);
}

#[test]
fn after_the_dot_the_reason_is_a_row_and_in_the_blended_list_it_is_silent() {
    let mut files = Files::new();
    let now = Instant::now();
    for status in [Status::NotRunning, Status::Alpha15, Status::TurnedOff] {
        let (rows, job) = files.rows(&query_for("pswds", true), Source::Unavailable(status), now);
        assert!(
            job.is_none(),
            "nothing is sent to an Everything that is not there"
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, status.title());
        assert_eq!(rows[0].tone, Tone::Warning);
        assert!(rows[0].enter.is_none());
        let (rows, job) = files.rows(&query_for("pswds", false), Source::Unavailable(status), now);
        assert!(rows.is_empty() && job.is_none());
    }
    // The hint is there at once after the prefix, before anything is typed.
    let (rows, _) = files.rows(
        &query_for("", true),
        Source::Unavailable(Status::NotRunning),
        now,
    );
    assert_eq!(
        rows[0].title,
        "File search needs Everything (voidtools). It is not running."
    );
    assert_eq!(rows[0].group, "Files");
}

#[test]
fn a_failed_search_is_told_after_the_dot_only() {
    let mut files = Files::new();
    let now = Instant::now();
    let (_, job) = files.rows(&query_for("pswds", true), Source::Everything, now);
    let job = job.expect("a new search is sent");
    files
        .client
        .receive(Incoming::Failed(job.id, Status::Loading));
    let (rows, job) = files.rows(&query_for("pswds", true), Source::Everything, now);
    assert!(job.is_none(), "the same text is not sent again");
    assert_eq!(rows[0].title, "Everything is still reading its index.");
    let (rows, _) = files.rows(&query_for("pswds", false), Source::Everything, now);
    assert!(rows.is_empty());
}

#[test]
fn a_new_text_gives_a_job_and_nothing_before_the_reply() {
    let mut files = Files::new();
    let (rows, job) = files.rows(
        &query_for("pswds", false),
        Source::Everything,
        Instant::now(),
    );
    assert!(rows.is_empty());
    let job = job.expect("sent");
    assert_eq!(job.text, "pswds");
    assert!(files.waiting());
    // One letter is too short to ask about.
    let (rows, job) = files.rows(&query_for("p", true), Source::Everything, Instant::now());
    assert!(rows.is_empty() && job.is_none());
}

#[test]
fn the_dot_is_the_prefix_of_the_file_search() {
    use crate::search::Router;
    use std::collections::BTreeMap;
    let router = Router::new(vec![(None, Box::new(Files::new()))], &BTreeMap::new());
    assert_eq!(router.split(".pswds"), Some((".".to_string(), "pswds")));
    assert_eq!(router.split("pswds"), None);
    // The settings file can move it or switch it off.
    let moved: BTreeMap<String, String> = [("files".to_string(), "ff".to_string())]
        .into_iter()
        .collect();
    let router = Router::new(vec![(None, Box::new(Files::new()))], &moved);
    assert_eq!(router.split("ffpswds"), Some(("ff".to_string(), "pswds")));
    assert_eq!(router.split(".pswds"), None);
}

/// Asks the Everything that runs on this PC. Read-only: it searches and
/// opens nothing. Run it with `cargo test -- --ignored live`.
#[test]
#[ignore = "needs Everything running with the pswds folder on D:"]
fn live_pswds_is_found() {
    let mut files = Files::new();
    let query = Query {
        text: "pswds".to_string(),
        prefix: None,
        limit: 8,
    };
    let context = test_context();
    let started = Instant::now();
    let mut rows = files.query(&query, &context);
    while rows.is_empty() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
        files.has_news();
        rows = files.query(&query, &context);
    }
    println!("answered in {} ms", started.elapsed().as_millis());
    for item in &rows {
        println!(
            "{} | {} | {} | {}",
            item.group, item.title, item.subtitle, item.score
        );
    }
    assert!(
        rows.iter().any(|item| {
            matches!(
                &item.enter,
                Some(Choice { action: Action::Open(path), .. })
                    if path.as_os_str().eq_ignore_ascii_case(r"D:\Windows_OneDrive\OneDrive\pswds_OneDrive")
            )
        }),
        "the pswds folder is in the rows"
    );
}

fn search_with(everything: bool, fallback: bool) -> SearchConfig {
    SearchConfig {
        everything,
        file_search_fallback: fallback,
        ..SearchConfig::default()
    }
}

#[test]
fn everything_answers_when_it_runs_and_the_index_when_it_does_not() {
    let both = search_with(true, true);
    assert_eq!(choose(&both, None, false), Source::Everything);
    assert_eq!(choose(&both, None, true), Source::Everything);
    for gone in [Status::NotRunning, Status::Alpha15] {
        assert_eq!(
            choose(&both, Some(gone), false),
            Source::Index { down: false }
        );
    }
    assert_eq!(
        choose(&both, Some(Status::NotRunning), true),
        Source::Index { down: true },
        "the index is still tried; a service that came back is found"
    );
}

#[test]
fn with_the_fallback_off_a_missing_everything_means_no_files() {
    let only = search_with(true, false);
    assert_eq!(choose(&only, None, false), Source::Everything);
    assert_eq!(
        choose(&only, Some(Status::NotRunning), false),
        Source::Unavailable(Status::NotRunning)
    );
    assert_eq!(
        choose(&only, Some(Status::Alpha15), false),
        Source::Unavailable(Status::Alpha15)
    );
}

#[test]
fn with_file_search_off_neither_source_is_asked() {
    for fallback in [true, false] {
        let off = search_with(false, fallback);
        for presence in [None, Some(Status::NotRunning)] {
            assert_eq!(
                choose(&off, presence, false),
                Source::Unavailable(Status::TurnedOff)
            );
        }
    }
}

#[test]
fn the_index_rows_say_where_they_come_from_and_keep_the_folder_first_order() {
    let mut files = Files::new();
    let Next::Send(id) = files.index.ask("report", Instant::now()) else {
        panic!()
    };
    files.index.receive(Incoming::Hits(
        id,
        vec![
            hit("report.docx", r"D:\a", false, 5),
            hit("old report archive", r"D:\a", true, 5),
        ],
    ));
    let source = Source::Index { down: false };
    let (rows, job) = files.rows(&query_for("report", false), source, Instant::now());
    assert!(job.is_none(), "the answer is in hand");
    let titles: Vec<&str> = rows.iter().map(|item| item.title.as_str()).collect();
    assert_eq!(titles, ["old report archive", "report.docx"]);
    assert_eq!(rows[0].group, "Folders");
    assert_eq!(rows[1].group, "Files");
    assert_eq!(rows[0].subtitle, "Windows index \u{b7} D:\\a");
    assert!(rows[0].tab_action.is_some() && rows[1].tab_action.is_none());
    let path = PathBuf::from(r"D:\a\report.docx");
    assert_eq!(
        rows[1].enter.as_ref().map(|c| &c.action),
        Some(&Action::Open(path))
    );
    // After the dot it is one list of up to fifty, as with Everything.
    let (rows, _) = files.rows(&query_for("report", true), source, Instant::now());
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|item| item.group == "Files and folders"));
    assert!(
        files.client.answer("report").is_none(),
        "Everything's client was not touched"
    );
}

#[test]
fn at_most_six_index_rows_in_the_blended_list_and_fifty_after_the_dot() {
    let mut files = Files::new();
    let Next::Send(id) = files.index.ask("proj", Instant::now()) else {
        panic!()
    };
    let mut hits: Vec<Hit> = (0..60)
        .map(|n| hit(&format!("proj{n:02}"), r"D:\a", true, n))
        .collect();
    hits.extend((0..60).map(|n| hit(&format!("proj{n:02}.txt"), r"D:\a", false, n)));
    files.index.receive(Incoming::Hits(id, hits));
    let source = Source::Index { down: false };
    let (rows, _) = files.rows(&query_for("proj", false), source, Instant::now());
    assert_eq!(rows.len(), 6);
    assert_eq!(
        rows.iter().filter(|item| item.group == "Folders").count(),
        4
    );
    let (rows, _) = files.rows(&query_for("proj", true), source, Instant::now());
    assert_eq!(rows.len(), 50);
}

#[test]
fn after_the_dot_the_index_says_so_and_in_the_blended_list_it_is_silent() {
    let mut files = Files::new();
    let now = Instant::now();
    let source = Source::Index { down: false };
    // Nothing typed yet, a single letter, and a search still running.
    for text in ["", "p", "pswds"] {
        let (rows, _) = files.rows(&query_for(text, true), source, now);
        assert_eq!(rows.len(), 1, "{text:?}");
        assert_eq!(
            rows[0].title,
            "Searching the Windows index (Everything is not running; it is faster)"
        );
        assert_eq!(rows[0].tone, Tone::Normal);
        assert!(rows[0].enter.is_none());
        let (rows, _) = files.rows(&query_for(text, false), source, now);
        assert!(rows.is_empty(), "{text:?}");
    }
}

#[test]
fn a_search_with_no_index_matches_keeps_the_notice() {
    let mut files = Files::new();
    let Next::Send(id) = files.index.ask("pswds", Instant::now()) else {
        panic!()
    };
    files.index.receive(Incoming::Hits(id, Vec::new()));
    let source = Source::Index { down: false };
    let (rows, job) = files.rows(&query_for("pswds", true), source, Instant::now());
    assert!(job.is_none());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, Status::UsingIndex.title());
    let (rows, _) = files.rows(&query_for("pswds", false), source, Instant::now());
    assert!(rows.is_empty());
}

#[test]
fn when_the_index_is_down_the_row_names_both_ways_to_search() {
    let mut files = Files::new();
    let now = Instant::now();
    let source = Source::Index { down: true };
    let (rows, job) = files.rows(&query_for("", true), source, now);
    assert!(job.is_none());
    assert_eq!(
        rows[0].title,
        "File search needs Everything or Windows Search"
    );
    assert_eq!(rows[0].tone, Tone::Warning);
    // A new text still goes out, so a service that came back is found.
    let (_, job) = files.rows(&query_for("pswds", true), source, now);
    let job = job.expect("sent");
    files.index.receive(incoming_of(windows_index::Done {
        id: job.id,
        outcome: Outcome::Unavailable,
    }));
    let (rows, job) = files.rows(&query_for("pswds", true), source, now);
    assert!(job.is_none());
    assert_eq!(
        rows[0].title,
        "File search needs Everything or Windows Search"
    );
    let (rows, _) = files.rows(&query_for("pswds", false), source, now);
    assert!(rows.is_empty());
}

#[test]
fn what_the_index_thread_found_becomes_hits_or_a_status() {
    let done = |outcome| windows_index::Done { id: 3, outcome };
    let found = vec![hit("a", r"D:\x", true, 1)];
    assert!(matches!(
        incoming_of(done(Outcome::Hits(found))),
        Incoming::Hits(3, hits) if hits.len() == 1
    ));
    for (outcome, status) in [
        (Outcome::Unavailable, Status::IndexUnavailable),
        (Outcome::Failed, Status::IndexFailed),
        (Outcome::TimedOut, Status::IndexTimedOut),
    ] {
        assert!(matches!(
            incoming_of(done(outcome)),
            Incoming::Failed(3, got) if got == status
        ));
    }
}

#[test]
fn a_slow_index_is_given_up_after_its_own_longer_wait() {
    let mut files = Files::new();
    let now = Instant::now();
    assert!(matches!(files.index.ask("pswds", now), Next::Send(_)));
    assert!(matches!(files.client.ask("pswds", now), Next::Send(_)));
    // Everything's wait is over; the index's is not.
    let later = now + GIVE_UP;
    files.client.expire(later);
    files.index.expire(later);
    assert_eq!(
        failure_of(&files.client, "pswds"),
        Some(Status::NotAnswering)
    );
    assert!(files.index.waiting(), "the index may take up to 800 ms");
    files.index.expire(now + Duration::from_millis(1020));
    assert!(!files.index.waiting());
    assert_eq!(
        failure_of(&files.index, "pswds"),
        Some(Status::IndexTimedOut)
    );
}

/// Asks the Windows index of this PC for "pswds", as the palette does
/// when Everything is not running, and reports what comes back and how
/// long it takes. Read-only: it searches and opens nothing, and it does
/// not look at Everything. Run it with
/// `cargo test -- --ignored live_index --nocapture`.
#[test]
#[ignore = "needs Windows Search running"]
fn live_index_pswds() {
    use super::super::oledb::Connection;
    let sql = windows_index::sql("pswds", windows_index::CANDIDATES).unwrap();
    println!("sql: {sql}");
    unsafe {
        windows_sys::Win32::System::Com::CoInitializeEx(
            std::ptr::null(),
            windows_sys::Win32::System::Com::COINIT_MULTITHREADED as u32,
        )
    };
    let opened = Instant::now();
    let connection = Connection::open().expect("the index opens");
    println!("opened in {} ms", opened.elapsed().as_millis());
    for round in 1..=2 {
        let asked = Instant::now();
        let rows = connection
            .query(
                &sql,
                windows_index::CANDIDATES,
                asked + Duration::from_secs(5),
            )
            .expect("the index answers");
        println!(
            "query {round}: {} rows in {} ms",
            rows.len(),
            asked.elapsed().as_millis()
        );
        for row in rows.iter().take(5) {
            println!("  {row:?}");
        }
    }

    let mut files = Files::new();
    let source = Source::Index { down: false };
    let query = query_for("pswds", false);
    let started = Instant::now();
    let (_, job) = files.rows(&query, source, started);
    let job = job.expect("a new search");
    windows_index::submit(job.id, &job.text);
    let mut rows = Vec::new();
    while started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
        files.has_news();
        if files.index.answer("pswds").is_some() || files.index.failure("pswds").is_some() {
            rows = files.rows(&query, source, Instant::now()).0;
            break;
        }
    }
    println!(
        "through the provider: {} rows in {} ms (the 120 ms wait for a pause in typing included), failure {:?}",
        rows.len(),
        started.elapsed().as_millis(),
        files.index.failure("pswds")
    );
    for item in &rows {
        println!(
            "{} | {} | {} | {}",
            item.group, item.title, item.subtitle, item.score
        );
    }
    assert_eq!(files.index.failure("pswds"), None);
    assert!(files.index.answer("pswds").is_some(), "the index answered");
}

/// Searches the Windows index of this PC for words that are likely to be
/// in it, to see rows come back whole: real paths, folders told from
/// files, Cyrillic and `_` kept. Read-only. Run it with
/// `cargo test -- --ignored live_index_words --nocapture`.
#[test]
#[ignore = "needs Windows Search running"]
fn live_index_words() {
    use super::super::oledb::Connection;
    unsafe {
        windows_sys::Win32::System::Com::CoInitializeEx(
            std::ptr::null(),
            windows_sys::Win32::System::Com::COINIT_MULTITHREADED as u32,
        )
    };
    let connection = Connection::open().expect("the index opens");
    for text in [
        "onedrive",
        "\u{437}\u{432}\u{456}\u{442}",
        "mods 41_51",
        "30%",
        "it's",
        "users admin zomboid",
        r"admin\zomboid",
    ] {
        let sql = windows_index::sql(text, windows_index::CANDIDATES).unwrap();
        let started = Instant::now();
        let rows = connection
            .query(
                &sql,
                windows_index::CANDIDATES,
                started + Duration::from_secs(5),
            )
            .expect("the index answers");
        println!(
            "{text:?}: {} rows in {} ms",
            rows.len(),
            started.elapsed().as_millis()
        );
        for row in rows.iter().take(3) {
            let hit = windows_index::hit_of(row);
            println!(
                "  {} | folder {:?} | {:?}",
                hit.as_ref().map_or(String::new(), Hit::full_path),
                hit.as_ref().map(|hit| hit.folder),
                row.modified
            );
        }
        assert!(rows.iter().all(|row| windows_index::hit_of(row).is_some()));
    }
}

#[test]
fn a_found_folder_can_be_bookmarked_with_ctrl_b() {
    let folder = hit("pswds_OneDrive", r"D:\Windows_OneDrive\OneDrive", true, 1);
    let item = row(
        Ranked {
            score: 1,
            hit: &folder,
        },
        "Folders",
        None,
    );
    assert_eq!(
        item.bookmark.map(|c| c.action),
        Some(Action::Provider {
            provider: "bookmarks".to_string(),
            command: format!(
                "new\t{}\tpswds_OneDrive",
                r"D:\Windows_OneDrive\OneDrive\pswds_OneDrive"
            ),
        })
    );
}
