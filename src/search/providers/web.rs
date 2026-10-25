use crate::search::{
    ask_ai, Action, Choice, Context, Glyph, IconRef, Query, ResultItem, SearchProvider,
};

pub const DEFAULT_URL: &str = "https://www.google.com/search?q={query}";

/// Asks the chosen AI website, or searches the web, in the default browser.
/// `?` with nothing after it is the list of prefixes, which the router
/// answers before this is asked. The addresses are read from the settings on
/// every query, so a change made in the settings window applies at once.
pub struct Web;

/// The address from the settings, or Google when it is not an http(s) one.
pub fn usable_url(url: &str) -> &str {
    if is_http(url) {
        url
    } else {
        DEFAULT_URL
    }
}

pub fn is_http(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

impl SearchProvider for Web {
    fn id(&self) -> &'static str {
        "web"
    }

    fn name(&self) -> &'static str {
        "Ask AI"
    }

    fn description(&self) -> &'static str {
        "Ask an AI website, or search the web, in your default browser"
    }

    fn default_prefix(&self) -> Option<&'static str> {
        Some("?")
    }

    fn placeholder(&self) -> &'static str {
        "Type a question for AI, or words to search the web for"
    }

    fn needle<'a>(&self, _text: &'a str) -> &'a str {
        ""
    }

    fn query(&mut self, query: &Query, context: &Context) -> Vec<ResultItem> {
        let words = query.text.trim();
        if words.is_empty() {
            return Vec::new();
        }
        let url = usable_url(&context.search.web_url);
        let mut rows = Vec::new();
        // An address typed in full can be opened as it is, and bookmarked.
        if is_http(words) && !words.contains(char::is_whitespace) {
            rows.push(ResultItem {
                group: "Web".to_string(),
                title: format!("Open {words}"),
                subtitle: host_of(words).to_string(),
                icon: IconRef::Glyph(Glyph::Globe),
                score: 1,
                enter: Some(Choice {
                    label: "Open".to_string(),
                    action: Action::OpenUrl(words.to_string()),
                }),
                bookmark: super::bookmarks::choice(words, words),
                ..Default::default()
            });
        }
        rows.push(ask_row(context, words));
        rows.push(ResultItem {
            group: "Web".to_string(),
            title: format!("Search the web for \u{201c}{words}\u{201d}"),
            subtitle: host_of(url).to_string(),
            icon: IconRef::Glyph(Glyph::Globe),
            enter: Some(Choice {
                label: "Search".to_string(),
                action: Action::OpenUrl(address(url, words)),
            }),
            ..Default::default()
        });
        rows
    }
}

/// "Ask Claude about …": Enter opens the AI website with the question,
/// Ctrl+Enter searches the web for it instead.
pub fn ask_row(context: &Context, words: &str) -> ResultItem {
    let site = ask_ai::site(context.search);
    ResultItem {
        group: "Web".to_string(),
        title: format!("Ask {} about \u{201c}{words}\u{201d}", site.name),
        subtitle: ask_ai::promise(context.search),
        icon: IconRef::Glyph(Glyph::Sparkles),
        enter: Some(Choice {
            label: "Ask".to_string(),
            action: Action::OpenUrl(ask_ai::url(context.search, words)),
        }),
        ctrl_enter: Some(search_choice(context, words)),
        ..Default::default()
    }
}

/// Searching the web for `words` with the address from the settings.
pub fn search_choice(context: &Context, words: &str) -> Choice {
    Choice {
        label: "Search the web".to_string(),
        action: Action::OpenUrl(address(usable_url(&context.search.web_url), words)),
    }
}

/// Without a `{query}` in the template the words go on the end, which is
/// what most search addresses expect.
pub fn address(template: &str, words: &str) -> String {
    let encoded = encode(words);
    if template.contains("{query}") {
        template.replace("{query}", &encoded)
    } else {
        format!("{template}{encoded}")
    }
}

pub fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    rest.split(['/', '?']).next().unwrap_or(rest)
}

/// Percent-encodes everything but the characters a URL never needs escaped.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_encoded_into_the_template() {
        assert_eq!(
            address(DEFAULT_URL, "rust egui & co"),
            "https://www.google.com/search?q=rust%20egui%20%26%20co"
        );
        assert_eq!(
            address("https://duckduckgo.com/?q=", "\u{0457}"),
            "https://duckduckgo.com/?q=%D1%97"
        );
    }

    #[test]
    fn the_subtitle_names_the_search_site() {
        assert_eq!(host_of(DEFAULT_URL), "www.google.com");
        assert_eq!(host_of("https://duckduckgo.com/?q="), "duckduckgo.com");
    }

    #[test]
    fn a_template_that_is_not_http_is_replaced_by_the_default() {
        assert_eq!(usable_url("file:///C:/evil.bat?{query}"), DEFAULT_URL);
        assert_eq!(
            usable_url("https://example.com/?q={query}"),
            "https://example.com/?q={query}"
        );
    }

    #[test]
    fn an_address_typed_in_full_can_be_opened_and_bookmarked() {
        let context = crate::search::test_context();
        let query = |text: &str| Query {
            text: text.to_string(),
            prefix: Some("?".to_string()),
            limit: 10,
        };
        let rows = Web.query(&query("https://example.com/docs"), &context);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].title, "Open https://example.com/docs");
        assert_eq!(
            rows[0].enter.as_ref().map(|c| &c.action),
            Some(&Action::OpenUrl("https://example.com/docs".to_string()))
        );
        assert!(rows[0].bookmark.is_some());
        assert!(rows[0].score > rows[1].score);
        // Words, or an address with words after it, are asked or searched for.
        assert_eq!(Web.query(&query("rust egui"), &context).len(), 2);
        assert_eq!(
            Web.query(&query("https://example.com a b"), &context).len(),
            2
        );
        assert!(Web.query(&query("rust"), &context)[0].bookmark.is_none());
    }

    #[test]
    fn a_question_is_asked_first_and_searched_with_ctrl_enter() {
        let context = crate::search::test_context();
        let rows = Web.query(
            &Query {
                text: "what is a PID".to_string(),
                prefix: Some("?".to_string()),
                limit: 10,
            },
            &context,
        );
        assert_eq!(
            rows[0].title,
            "Ask Claude about \u{201c}what is a PID\u{201d}"
        );
        assert_eq!(
            rows[0].subtitle,
            "Opens Claude in your browser with this question."
        );
        assert_eq!(
            rows[0].enter.as_ref().map(|c| &c.action),
            Some(&Action::OpenUrl(
                "https://claude.ai/new?q=what%20is%20a%20PID".to_string()
            ))
        );
        assert_eq!(
            rows[0].ctrl_enter.as_ref().map(|c| &c.action),
            Some(&Action::OpenUrl(
                "https://www.google.com/search?q=what%20is%20a%20PID".to_string()
            ))
        );
        assert_eq!(
            rows[1].title,
            "Search the web for \u{201c}what is a PID\u{201d}"
        );
    }
}
