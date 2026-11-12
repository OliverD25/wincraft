//! Claude's text made fit for one line of a list and a small panel: no
//! code blocks, no Markdown marks, and nothing that looks like a secret.

/// What a word that looks like a secret becomes.
pub const HIDDEN: &str = "[hidden]";
/// A word this long, with letters and digits and no spaces, looks like a
/// token or a key.
const TOKEN_LEN: usize = 24;
/// Words that start like a known kind of key are hidden from this length.
const KEY_LEN: usize = 12;
const KEY_STARTS: [&str; 8] = [
    "sk-",
    "ghp_",
    "gho_",
    "ghs_",
    "github_pat_",
    "xox",
    "AKIA",
    "eyJ",
];
/// An inline code span longer than this is likely a command, not a name.
const LONGEST_CODE: usize = 40;

/// The text without code blocks, tables and Markdown marks, with secrets
/// hidden. Paragraphs are kept apart by one line break; the lines inside
/// a paragraph are joined.
pub fn clean(text: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut in_code = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if in_code || line.starts_with('|') {
            continue;
        }
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
            continue;
        }
        let line = plain_line(line);
        if !line.is_empty() {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }
    paragraphs.join("\n")
}

/// One line without its Markdown marks, its long code spans and secrets.
fn plain_line(line: &str) -> String {
    let mut line = line.trim_start_matches('#').trim_start_matches('>').trim();
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            line = rest;
        }
    }
    let line = code_spans(line);
    let line = links(&line).replace("**", "").replace("__", "");
    line.split_whitespace()
        .map(scrub)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Backticks dropped; a long code span becomes an ellipsis.
fn code_spans(line: &str) -> String {
    let mut out = String::new();
    let mut parts = line.split('`');
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for (at, part) in parts.enumerate() {
        let inside = at % 2 == 0;
        if inside && part.chars().count() > LONGEST_CODE {
            out.push('\u{2026}');
        } else {
            out.push_str(part);
        }
    }
    out
}

/// `[words](address)` becomes `words`.
fn links(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find("](") else {
            break;
        };
        let target = &after[close + 2..];
        let Some(end) = target.find(')') else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&after[..close]);
        rest = &target[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A word, or `[hidden]` when it looks like a secret: a long run of
/// letters and digits, a known key's start, or `NAME=value`.
fn scrub(word: &str) -> String {
    let core = word.trim_matches(|c: char| "\"'()[]{}<>,;!?".contains(c));
    if let Some((name, value)) = core.split_once('=') {
        let is_name = name.len() >= 2
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if is_name && !value.is_empty() {
            return word.replacen(core, &format!("{name}={HIDDEN}"), 1);
        }
    }
    let core = core.trim_end_matches(['.', ':']);
    let chars = core.chars().count();
    let token_like = core
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_+/=.:".contains(c));
    let mixed =
        core.chars().any(|c| c.is_ascii_digit()) && core.chars().any(|c| c.is_ascii_alphabetic());
    let keyish = chars >= KEY_LEN && KEY_STARTS.iter().any(|start| core.starts_with(start));
    if token_like && ((chars >= TOKEN_LEN && mixed) || keyish) {
        return word.replacen(core, HIDDEN, 1);
    }
    word.to_string()
}

/// The text cut to `most` characters, with an ellipsis when cut.
pub fn cap(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let cut: String = text.chars().take(most.saturating_sub(1)).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// Where a sentence ends: after `.`, `!` or `?` followed by a space.
fn sentence_ends(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    (0..bytes.len())
        .filter(|at| {
            matches!(bytes[*at], b'.' | b'!' | b'?')
                && bytes.get(at + 1).is_none_or(|next| *next == b' ')
        })
        .map(|at| at + 1)
        .collect()
}

/// The first sentence of the first paragraph.
pub fn first_sentence(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    let end = sentence_ends(first)
        .into_iter()
        .find(|end| *end >= 8)
        .unwrap_or(first.len());
    first[..end].trim().to_string()
}

/// The last sentence that ends with a question mark, if any.
pub fn last_question(text: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let ends = sentence_ends(line);
        let at = ends
            .iter()
            .rposition(|end| line.as_bytes()[end - 1] == b'?')?;
        let start = if at == 0 { 0 } else { ends[at - 1] };
        let question = line[start..ends[at]].trim();
        (!question.is_empty()).then(|| question.to_string())
    })
}
