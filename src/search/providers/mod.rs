mod apps;
mod calc;
mod commands;
mod drives;
mod paths;
pub mod web;
mod windows;

use crate::core::config::SearchConfig;
use crate::search::SearchProvider;

/// The providers WinCraft ships, in the order their rows are merged.
pub fn built_in(config: &SearchConfig) -> Vec<Box<dyn SearchProvider>> {
    vec![
        Box::new(commands::Commands),
        Box::new(apps::Apps::new()),
        Box::new(windows::Windows::default()),
        Box::new(paths::Paths::default()),
        Box::new(calc::Calculator),
        Box::new(web::Web::new(&config.web_url)),
    ]
}
