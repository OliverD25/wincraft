mod apps;
mod commands;
mod windows;

use crate::search::SearchProvider;

/// The providers WinCraft ships, in the order their rows are merged.
pub fn built_in() -> Vec<Box<dyn SearchProvider>> {
    vec![
        Box::new(commands::Commands),
        Box::new(apps::Apps::new()),
        Box::new(windows::Windows::default()),
    ]
}
