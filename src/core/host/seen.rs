//! "Mark all as seen": clears the orange marks of WindowDeck and, for each
//! marked window, the orange of its taskbar button.

use super::*;
use crate::core::attention;
use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;

impl Host {
    /// Ctrl+M in the strip (`from_strip`) or the palette command. The strip
    /// answers on its own screen, so the palette gets a balloon instead.
    pub(super) fn mark_all_seen(&mut self, from_strip: bool) {
        let marked = ATTENTION.with(|attention| attention.borrow_mut().clear_all());
        let stopped = attention::stop_flashing(&marked, crate::core::instance::is_test());
        log::info!("attention: marked {} windows as seen", marked.len());
        crate::detailed!(
            "attention: marked as seen ({stopped} taskbar buttons told to stop): {}",
            marked
                .iter()
                .map(|hwnd| format!(
                    "{hwnd:#x} \"{}\"",
                    crate::core::windows_list::window_text(*hwnd as HWND)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
        if !marked.is_empty() {
            refresh_arrange();
        }
        if from_strip {
            self.to_ui.send(UiCommand::ArrangeMarked {
                windows: marked,
                undone: false,
            });
        } else {
            self.tray.balloon(
                "WinCraft",
                &format!("{}.", attention::marked_line(marked.len())),
            );
        }
    }

    /// Ctrl+Z on that line: the marks come back on the windows that still
    /// exist. The taskbar buttons stay as they are.
    pub(super) fn restore_marks(&mut self, windows: &[isize]) {
        let alive: Vec<isize> = windows
            .iter()
            .copied()
            .filter(|hwnd| unsafe { IsWindow(*hwnd as HWND) } != 0)
            .collect();
        ATTENTION.with(|attention| attention.borrow_mut().restore(&alive));
        log::info!("attention: put the marks back on {} windows", alive.len());
        refresh_arrange();
        self.to_ui.send(UiCommand::ArrangeMarked {
            windows: Vec::new(),
            undone: true,
        });
    }
}
