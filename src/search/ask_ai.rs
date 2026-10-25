//! Ask AI opens the person's chosen AI website in the default browser with
//! the question filled in. WinCraft itself talks to no AI service and keeps
//! no account or key (Principle 5); the answer is read on that website.

use crate::core::config::SearchConfig;
use crate::search::providers::web;

pub const DEFAULT_SITE: &str = "Claude";
pub const CUSTOM: &str = "Custom URL";

/// The sites Settings offers, with the address each takes a question at.
pub const SITES: [(&str, &str); 4] = [
    ("Claude", "https://claude.ai/new?q="),
    ("ChatGPT", "https://chatgpt.com/?q="),
    ("Perplexity", "https://www.perplexity.ai/search?q="),
    ("Copilot", "https://copilot.microsoft.com/?q="),
];

/// The names in Settings' list, the custom address last.
pub const SITE_NAMES: [&str; 5] = ["Claude", "ChatGPT", "Perplexity", "Copilot", CUSTOM];

/// The site a question goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    /// What the palette calls it: "Claude", or the custom address's host.
    pub name: String,
    pub template: String,
}

/// Why a custom address cannot be used, or None when it can.
pub fn custom_problem(template: &str) -> Option<&'static str> {
    let template = template.trim();
    if template.is_empty() {
        Some("Type the address of the AI website.")
    } else if !web::is_http(template) {
        Some("It must start with https:// or http://.")
    } else {
        None
    }
}

/// The chosen site, or Claude when the choice is unknown or the custom
/// address cannot be used.
pub fn site(search: &SearchConfig) -> Site {
    if search.ask_ai_site == CUSTOM && custom_problem(&search.ask_ai_custom).is_none() {
        let template = search.ask_ai_custom.trim().to_string();
        return Site {
            name: web::host_of(&template).to_string(),
            template,
        };
    }
    let (name, template) = SITES
        .iter()
        .find(|(name, _)| *name == search.ask_ai_site)
        .unwrap_or(&SITES[0]);
    Site {
        name: name.to_string(),
        template: template.to_string(),
    }
}

/// The address that opens the site with `question` typed in, encoded.
pub fn url(search: &SearchConfig, question: &str) -> String {
    web::address(&site(search).template, question.trim())
}

/// What the inspector says before the question is sent anywhere.
pub fn promise(search: &SearchConfig) -> String {
    format!(
        "Opens {} in your browser with this question.",
        site(search).name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(site: &str, custom: &str) -> SearchConfig {
        SearchConfig {
            ask_ai_site: site.to_string(),
            ask_ai_custom: custom.to_string(),
            ..SearchConfig::default()
        }
    }

    #[test]
    fn the_question_is_encoded_into_the_chosen_site() {
        let claude = SearchConfig::default();
        assert_eq!(
            url(&claude, " what is a PID? "),
            "https://claude.ai/new?q=what%20is%20a%20PID%3F"
        );
        assert_eq!(
            url(&config("ChatGPT", ""), "a&b"),
            "https://chatgpt.com/?q=a%26b"
        );
        assert_eq!(
            url(&config("Perplexity", ""), "\u{0457}"),
            "https://www.perplexity.ai/search?q=%D1%97"
        );
        assert_eq!(
            url(&config("Copilot", ""), "x"),
            "https://copilot.microsoft.com/?q=x"
        );
    }

    #[test]
    fn a_custom_template_takes_the_question_where_query_is() {
        let custom = config(CUSTOM, "https://ai.example.com/chat?prompt={query}&new=1");
        assert_eq!(
            url(&custom, "two words"),
            "https://ai.example.com/chat?prompt=two%20words&new=1"
        );
        assert_eq!(site(&custom).name, "ai.example.com");
        // Without {query} the question goes on the end.
        assert_eq!(
            url(&config(CUSTOM, "https://ai.example.com/?q="), "x"),
            "https://ai.example.com/?q=x"
        );
    }

    #[test]
    fn an_unusable_choice_falls_back_to_claude() {
        assert_eq!(site(&config(CUSTOM, "")).name, "Claude");
        assert_eq!(
            site(&config(CUSTOM, "file:///C:/x.bat?{query}")).name,
            "Claude"
        );
        assert_eq!(site(&config("Nonsense", "")).name, "Claude");
        assert_eq!(
            custom_problem("ftp://x"),
            Some("It must start with https:// or http://.")
        );
        assert_eq!(custom_problem("https://x/?q={query}"), None);
    }

    #[test]
    fn the_promise_names_the_site() {
        assert_eq!(
            promise(&config("ChatGPT", "")),
            "Opens ChatGPT in your browser with this question."
        );
    }

    #[test]
    fn every_site_in_the_list_but_custom_has_an_address() {
        for name in SITE_NAMES {
            if name != CUSTOM {
                assert_eq!(site(&config(name, "")).name, name);
            }
        }
    }
}
