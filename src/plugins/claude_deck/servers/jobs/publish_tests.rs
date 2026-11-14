//! The snapshot for WindowDeck's Claude board.

use super::publish::entries;
use super::Job;

fn job(server: &str, chats: &[&str], started: Option<u64>, cpu10: Option<u32>) -> Job {
    Job {
        server: server.to_string(),
        chats: chats
            .iter()
            .map(|id| (id.to_string(), format!("{id} title")))
            .collect(),
        started,
        cpu10,
        ..Job::default()
    }
}

#[test]
fn each_chat_gets_its_servers_count_oldest_start_and_busy_flag() {
    let jobs = vec![
        job("rdehome2", &["a"], Some(500), Some(10)),
        job("rdeoffice1", &["a"], Some(200), Some(400)),
        job("rdehome2", &["a"], None, None),
        job("rdehome2", &["b"], Some(900), None),
        job("rdehome2", &["c", "d"], Some(100), Some(900)),
    ];
    let got = entries(&jobs);
    assert_eq!(got.len(), 2, "the repo's job marks no chat");
    assert_eq!(got[0].chat_id, "a");
    assert_eq!(got[0].servers.len(), 2);
    assert_eq!(got[0].jobs, 3);
    assert_eq!(got[0].oldest, Some(200));
    assert!(got[0].busy);
    assert_eq!(got[1].chat_id, "b");
    assert_eq!(got[1].jobs, 1);
    assert!(!got[1].busy);
}
