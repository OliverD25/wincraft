pub mod screen_dimmer;
pub mod shortcut_detector;

use crate::core::traits::WinCraftPlugin;

/// Every plugin in the program is listed here. Adding one is a single line.
pub fn load_active_plugins() -> Vec<Box<dyn WinCraftPlugin>> {
    vec![
        Box::new(screen_dimmer::ScreenDimmer::new()),
        Box::new(shortcut_detector::ShortcutDetector::new()),
    ]
}
