//! Every **label** in docs/ui-map.md must still be in the source, so the map
//! never names a button or menu that no longer exists.

use std::fs;
use std::path::Path;

/// The bold labels of the map with their line numbers. A label followed by
/// ` (not ours)` is skipped.
fn labels(map: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in map.lines().enumerate() {
        let mut rest = line;
        while let Some(start) = rest.find("**") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("**") else {
                break;
            };
            let label = &after[..end];
            rest = &after[end + 2..];
            if rest.starts_with(" (not ours)") || label.is_empty() {
                continue;
            }
            found.push((index + 1, normalize(label)));
        }
    }
    found
}

/// Turns `\u{2026}` escapes into the character and `...` into the ellipsis.
fn normalize(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("\\u{") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 3..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[at..]);
            return out.replace("...", "\u{2026}");
        };
        match u32::from_str_radix(&after[..close], 16)
            .ok()
            .and_then(char::from_u32)
        {
            Some(ch) => out.push(ch),
            None => out.push_str(&rest[at..at + 3 + close + 1]),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out.replace("...", "\u{2026}")
}

fn rust_files(dir: &Path, into: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, into);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            if let Ok(text) = fs::read_to_string(&path) {
                into.push(normalize(&text));
            }
        }
    }
}

/// The parts of a label around its `{...}` wildcards, in order.
fn parts(label: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = label;
    while let Some(start) = rest.find('{') {
        parts.push(&rest[..start]);
        match rest[start..].find('}') {
            Some(end) => rest = &rest[start + end + 1..],
            None => return parts,
        }
    }
    parts.push(rest);
    parts
}

/// Whether the parts appear in order in one line (a label with a wildcard)
/// or anywhere in the file (a plain label).
fn found_in(source: &str, parts: &[&str]) -> bool {
    if parts.len() == 1 {
        return source.contains(parts[0]);
    }
    source.lines().any(|line| {
        let mut from = 0;
        parts
            .iter()
            .filter(|part| !part.is_empty())
            .all(|part| match line[from..].find(part) {
                Some(at) => {
                    from += at + part.len();
                    true
                }
                None => false,
            })
    })
}

#[test]
fn every_label_in_the_ui_map_is_in_the_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let map = fs::read_to_string(root.join("docs/ui-map.md")).expect("docs/ui-map.md");
    let mut sources = Vec::new();
    rust_files(&root.join("src"), &mut sources);
    assert!(!sources.is_empty(), "no .rs files under src/");

    let missing: Vec<String> = labels(&map)
        .into_iter()
        .filter(|(_, label)| {
            let parts = parts(label);
            !sources.iter().any(|source| found_in(source, &parts))
        })
        .map(|(line, label)| format!("  docs/ui-map.md:{line}: **{label}**"))
        .collect();
    assert!(
        missing.is_empty(),
        "labels in docs/ui-map.md that are not in src/ (fix the map or the code):\n{}",
        missing.join("\n")
    );
}

#[test]
fn the_label_reader_follows_the_format_rules() {
    let found = labels("a **Plain** and **Skip me** (not ours) and **Dots...**\n**Two**");
    let texts: Vec<_> = found.iter().map(|(_, text)| text.as_str()).collect();
    assert_eq!(texts, ["Plain", "Dots\u{2026}", "Two"]);
    assert_eq!(found[2].0, 2);
    assert_eq!(parts("Update to {version}"), ["Update to ", ""]);
    assert!(found_in("format!(\"Update to {}\", x)", &["Update to ", ""]));
    assert!(!found_in("\"Update\"", &["Update to ", ""]));
}
