use crate::search::{Action, Choice, Context, Glyph, IconRef, Query, ResultItem, SearchProvider};

pub const DEFAULT_URL: &str = "https://www.google.com/search?q={query}";

/// Searches the web in the default browser. `?` with nothing after it is
/// the list of prefixes, which the router answers before this is asked.
pub struct Web {
    /// The search address with `{query}` where the words go.
    url: String,
}

impl Web {
    pub fn new(url: &str) -> Self {
        let url = if url.starts_with("https://") || url.starts_with("http://") {
            url.to_string()
        } else {
            log::warn!("search.web_url {url:?} is not an http address; using {DEFAULT_URL}");
            DEFAULT_URL.to_string()
        };
        Self { url }
    }
}

impl SearchProvider for Web {
    fn id(&self) -> &'static str {
        "web"
    }

    fn name(&self) -> &'static str {
        "Web search"
    }

    fn description(&self) -> &'static str {
        "Search the web in your default browser"
    }

    fn default_prefix(&self) -> Option<&'static str> {
        Some("?")
    }

    fn placeholder(&self) -> &'static str {
        "Type what to search the web for"
    }

    fn needle<'a>(&self, _text: &'a str) -> &'a str {
        ""
    }

    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        let words = query.text.trim();
        if words.is_empty() {
            return Vec::new();
        }
        vec![ResultItem {
            group: "Web".to_string(),
            title: format!("Search the web for \u{201c}{words}\u{201d}"),
            subtitle: host_of(&self.url).to_string(),
            icon: IconRef::Glyph(Glyph::Globe),
            enter: Some(Choice {
                label: "Search".to_string(),
                action: Action::OpenUrl(address(&self.url, words)),
            }),
            ..Default::default()
        }]
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

fn host_of(url: &str) -> &str {
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
        assert_eq!(Web::new("file:///C:/evil.bat?{query}").url, DEFAULT_URL);
        assert_eq!(
            Web::new("https://example.com/?q={query}").url,
            "https://example.com/?q={query}"
        );
    }
}
