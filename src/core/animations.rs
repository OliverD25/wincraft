//! The user's Windows setting "Animation effects" (Settings, Accessibility,
//! Visual effects). WinCraft's motion follows it: with it off, things
//! change at once instead of sliding or fading.

/// Whether "Animation effects" is on. When Windows cannot say, it is on.
pub fn on() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
    };
    let mut on: i32 = 1;
    // SAFETY: the call writes one BOOL into `on`.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut on as *mut i32).cast(),
            0,
        )
    };
    read == 0 || on != 0
}
