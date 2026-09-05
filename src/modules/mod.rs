pub mod screen_dimmer;

use crate::core::traits::WinCraftModule;

/// Every module in the program is listed here. Adding one is a single line.
pub fn load_active_modules() -> Vec<Box<dyn WinCraftModule>> {
    vec![Box::new(screen_dimmer::ScreenDimmer::new())]
}
