use super::*;
use crate::core::release_cache::Answer;
use std::io::{Read, Write};
use std::net::TcpListener;

/// A one-request server on this PC that answers with `head` (status line
/// and headers) and returns the request it got. Never the internet.
fn serve_once(head: &'static str) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 2048];
        let read = stream.read(&mut request).unwrap_or(0);
        stream.write_all(head.as_bytes()).unwrap();
        String::from_utf8_lossy(&request[..read]).into_owned()
    });
    (format!("http://127.0.0.1:{port}"), server)
}

#[test]
fn the_redirect_names_the_tag() {
    let tag = |location: &str| tag_from_location(location);
    assert_eq!(
        tag("https://github.com/OliverD25/wincraft/releases/tag/v0.20.68"),
        Ok(Some("v0.20.68".to_string()))
    );
    assert_eq!(
        tag("/OliverD25/wincraft/releases/tag/v0.20.68/"),
        Ok(Some("v0.20.68".to_string()))
    );
    assert_eq!(
        tag("https://github.com/OliverD25/wincraft/releases/tag/v1.0.0-beta.1?x=1"),
        Ok(Some("v1.0.0-beta.1".to_string()))
    );
    assert_eq!(
        tag("https://github.com/OliverD25/wincraft/releases"),
        Ok(None),
        "no release published yet"
    );
}

#[test]
fn a_junk_redirect_is_an_error() {
    for junk in [
        "",
        "https://github.com/login",
        "https://github.com/OliverD25/wincraft/releases/tag/",
        "https://github.com/OliverD25/wincraft/releases/tag/nightly",
        "https://github.com/OliverD25/wincraft/releases/tag/v0.20.68/../../evil",
        "https://github.com/OliverD25/wincraft/releases/tag/v0.20<script>",
    ] {
        assert!(tag_from_location(junk).is_err(), "{junk}");
    }
}

#[test]
fn a_release_known_by_its_tag_downloads_from_the_fixed_addresses() {
    let release = release_from_tag("v0.20.69", RELEASES_PAGE_LATEST, None);
    let urls: Vec<&str> = release.assets.iter().map(|a| a.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://github.com/OliverD25/wincraft/releases/download/v0.20.69/wincraft.exe",
            "https://github.com/OliverD25/wincraft/releases/download/v0.20.69/SHA256SUMS.txt",
        ]
    );
    for url in urls {
        assert!(crate::core::update::allowed_asset(url, false), "{url}");
    }
    assert!(release.notes.is_empty());
    assert!(release.page.ends_with("/releases/tag/v0.20.69"));
    let fake = release_from_tag("v9.9.9", "http://127.0.0.1:5000/releases/latest", Some("n"));
    assert_eq!(
        fake.assets[0].url,
        "http://127.0.0.1:5000/releases/download/v9.9.9/wincraft.exe"
    );
    assert_eq!(fake.notes, "n");
}

#[test]
fn the_limit_headers_say_how_long_to_wait() {
    let now = 1_800_000_000;
    assert_eq!(limit_wait(Some("120"), Some("1800000600"), now), Some(120));
    assert_eq!(limit_wait(None, Some("1800000600"), now), Some(600));
    assert_eq!(limit_wait(Some("soon"), Some("1800000600"), now), Some(600));
    assert_eq!(limit_wait(None, Some("1700000000"), now), Some(0), "past");
    assert_eq!(
        limit_wait(None, Some("2800000000"), now),
        Some(LONGEST_WAIT)
    );
    assert_eq!(limit_wait(None, None, now), None);
    assert_eq!(minutes(Some(600)), 10);
    assert_eq!(minutes(Some(61)), 2);
    assert_eq!(minutes(Some(0)), 1);
    assert_eq!(minutes(None), 60);
}

#[test]
fn a_403_from_the_api_is_its_limit_not_offline() {
    let (base, server) = serve_once(
        "HTTP/1.1 403 Forbidden\r\nx-ratelimit-remaining: 0\r\nretry-after: 90\r\n\
         Content-Length: 0\r\nConnection: close\r\n\r\n",
    );
    let answer = github::get_if_changed(&format!("{base}/releases/latest"), None);
    server.join().unwrap();
    assert_eq!(answer, Err(CheckError::Limited { wait: Some(90) }));
}

#[test]
fn a_429_is_the_limit_too_and_other_statuses_say_their_number() {
    let (base, server) =
        serve_once("HTTP/1.1 429 Too Many\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let answer = github::get_if_changed(&format!("{base}/x"), None);
    server.join().unwrap();
    assert_eq!(answer, Err(CheckError::Limited { wait: None }));

    let (base, server) =
        serve_once("HTTP/1.1 500 Oops\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let answer = github::get_if_changed(&format!("{base}/x"), None);
    server.join().unwrap();
    assert_eq!(
        answer,
        Err(CheckError::Other("http status: 500".to_string()))
    );

    let (base, server) =
        serve_once("HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let answer = github::get_if_changed(&format!("{base}/x"), Some("\"abc\""));
    let request = server.join().unwrap();
    assert_eq!(answer, Ok(Answer::NotModified));
    assert!(request
        .to_ascii_lowercase()
        .contains("if-none-match: \"abc\""));
}

#[test]
fn nobody_listening_is_offline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let answer = github::get_if_changed(&format!("http://127.0.0.1:{port}/x"), None);
    assert!(matches!(answer, Err(CheckError::Offline(_))), "{answer:?}");
}

#[test]
fn the_releases_page_is_asked_with_head_and_its_redirect_is_not_followed() {
    let (base, server) = serve_once(
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/OliverD25/wincraft/releases/tag/v0.20.70\r\n\
         Content-Length: 0\r\nConnection: close\r\n\r\n",
    );
    let tag = latest_tag_by_redirect(&format!("{base}/releases/latest"));
    let request = server.join().unwrap();
    assert_eq!(tag, Ok(Some("v0.20.70".to_string())));
    assert!(request.starts_with("HEAD /releases/latest"), "{request}");
}

#[test]
fn a_releases_page_that_does_not_redirect_is_an_error() {
    let (base, server) =
        serve_once("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let tag = latest_tag_by_redirect(&format!("{base}/releases/latest"));
    server.join().unwrap();
    assert!(matches!(tag, Err(CheckError::Other(_))), "{tag:?}");
}
