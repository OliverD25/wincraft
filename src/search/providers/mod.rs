pub mod apps;
mod apps_folder;
pub mod bookmarks;
mod calc;
mod commands;
mod danger;
mod drives;
mod everything_ipc;
mod explorer;
mod files;
mod installed;
mod oledb;
mod paths;
mod process;
pub mod shell;
mod terminal;
pub mod web;
mod windows;
mod windows_index;

use crate::search::SearchProvider;

/// The providers WinCraft ships, in the order their rows are merged.
pub fn built_in() -> Vec<Box<dyn SearchProvider>> {
    vec![
        Box::new(commands::Commands),
        Box::new(apps::Apps::new()),
        Box::new(windows::Windows::default()),
        Box::new(bookmarks::Bookmarks::new()),
        Box::new(paths::Paths::default()),
        Box::new(files::Files::new()),
        Box::new(calc::Calculator),
        Box::new(terminal::Terminal::new()),
        Box::new(web::Web),
    ]
}
