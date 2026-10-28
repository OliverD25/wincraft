//! The tools that are not a switch: copying the PC's address or a new
//! UUID, plain text on the clipboard, the DNS cache, the microphone, the
//! Recycle Bin and the Environment Variables window. All through `System`.

use super::actions::{failed, Outcome, Undo};
use super::system::System;
use crate::core::notice::Glyph;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    CopyIp,
    FlushDns,
    PlainText,
    NewUuid,
    Microphone,
    EmptyBin,
    EnvVars,
}

/// A version 4 UUID from 16 random bytes, in the usual lower-case form.
pub fn uuid_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// "340 MB", "1.2 GB", "512 bytes".
pub fn size_text(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} bytes")
    } else if value < KB * KB {
        format!("{:.0} KB", value / KB)
    } else if value < KB * KB * KB {
        format!("{:.0} MB", value / (KB * KB))
    } else {
        format!("{:.1} GB", value / (KB * KB * KB))
    }
}

pub fn items_text(count: u64) -> String {
    if count == 1 {
        "1 item".to_string()
    } else {
        format!("{count} items")
    }
}

/// What the Recycle Bin holds, for the notice and the inspector.
pub fn bin_text(items: u64, bytes: u64) -> String {
    format!("{}, {}", items_text(items), size_text(bytes))
}

/// The start of a text on one line, for the inspector.
pub fn preview(text: &str, room: usize) -> String {
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= room {
        line
    } else {
        let cut: String = line.chars().take(room.saturating_sub(1)).collect();
        format!("{}\u{2026}", cut.trim_end())
    }
}

/// The UUID the next Copy New UUID copies: the one the inspector showed,
/// or a new one.
pub fn next_uuid(system: &dyn System, uuid: &mut Option<String>) -> Result<String, String> {
    if let Some(known) = uuid {
        return Ok(known.clone());
    }
    let made = uuid_v4(system.random()?);
    *uuid = Some(made.clone());
    Ok(made)
}

fn done(glyph: Glyph, text: &str, detail: String) -> Outcome {
    log::info!("QuickActions: {text}");
    Outcome::new(glyph, text.to_string(), Some(detail))
}

pub fn run(tool: Tool, system: &mut dyn System, uuid: &mut Option<String>) -> Option<Outcome> {
    Some(match tool {
        Tool::CopyIp => {
            let addresses = system.local_addresses();
            let Some(first) = addresses.first() else {
                return Some(Outcome::new(
                    Glyph::Info,
                    "No network address found".to_string(),
                    Some("This PC is not connected to a network.".to_string()),
                ));
            };
            match system.put_clipboard(&first.ip) {
                Ok(()) => done(
                    Glyph::Check,
                    &format!("Copied {}", first.ip),
                    format!("This PC's {} address", first.kind.name()),
                ),
                Err(reason) => failed("The clipboard", reason),
            }
        }
        Tool::FlushDns => match system.flush_dns() {
            Ok(()) => done(
                Glyph::Check,
                "DNS cache cleared",
                "Windows looks up web addresses afresh.".to_string(),
            ),
            Err(reason) => failed("The DNS cache", reason),
        },
        Tool::PlainText => {
            let Some(text) = system.clipboard_text() else {
                return Some(Outcome::new(
                    Glyph::Info,
                    "The clipboard has no text".to_string(),
                    Some("Copy some text first, then run it again.".to_string()),
                ));
            };
            match system.put_clipboard(&text) {
                Ok(()) => done(
                    Glyph::Check,
                    "The clipboard holds plain text now",
                    format!("{} characters, without formatting", text.chars().count()),
                ),
                Err(reason) => failed("The clipboard", reason),
            }
        }
        Tool::NewUuid => {
            let made = match next_uuid(system, uuid) {
                Ok(made) => made,
                Err(reason) => return Some(failed("The clipboard", reason)),
            };
            match system.put_clipboard(&made) {
                Ok(()) => {
                    // The next one is new, and the inspector shows it.
                    *uuid = None;
                    done(Glyph::Check, "Copied a new UUID", made)
                }
                Err(reason) => failed("The clipboard", reason),
            }
        }
        Tool::Microphone => {
            let (muted, name) = match system.microphone() {
                Ok(state) => state,
                Err(reason) => return Some(failed("The microphone", reason)),
            };
            match system.set_microphone_muted(!muted) {
                Ok(()) => {
                    let text = if muted {
                        "Microphone on"
                    } else {
                        "Microphone muted"
                    };
                    done(Glyph::Check, text, name).undo(Undo::Microphone(muted))
                }
                Err(reason) => failed("The microphone", reason),
            }
        }
        Tool::EmptyBin => {
            let Some((items, bytes)) = system.recycle_bin() else {
                return Some(failed(
                    "The Recycle Bin",
                    "Windows did not say what is in it".to_string(),
                ));
            };
            if items == 0 {
                return Some(Outcome::new(
                    Glyph::Info,
                    "The Recycle Bin is already empty".to_string(),
                    None,
                ));
            }
            let what = bin_text(items, bytes);
            match system.empty_recycle_bin() {
                Ok(true) => done(Glyph::Trash, "Recycle Bin emptied", what),
                Ok(false) => Outcome::new(
                    Glyph::Loader,
                    "Emptying the Recycle Bin\u{2026}".to_string(),
                    Some(what),
                ),
                Err(reason) => failed("The Recycle Bin", reason),
            }
        }
        Tool::EnvVars => match system.open_environment_variables() {
            Ok(()) => return None,
            Err(reason) => Outcome::new(
                Glyph::Warning,
                "Environment Variables did not open".to_string(),
                Some(reason),
            ),
        },
    })
}

/// The notice after the Recycle Bin finished in the background. `what` is
/// what it held.
pub fn bin_finished(ok: bool, what: &str) -> Outcome {
    if ok {
        done(Glyph::Trash, "Recycle Bin emptied", what.to_string())
    } else {
        failed(
            "The Recycle Bin",
            "Windows could not empty it. A file in it may be in use.".to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::network::{Address, Kind};
    use super::super::system::tests::Fake;
    use super::*;

    fn run_tool(tool: Tool, fake: &mut Fake) -> Outcome {
        run(tool, fake, &mut None).expect("an outcome")
    }

    #[test]
    fn a_uuid_is_version_4_in_the_usual_form() {
        let made = uuid_v4([0xff; 16]);
        assert_eq!(made, "ffffffff-ffff-4fff-bfff-ffffffffffff");
        let made = uuid_v4([0; 16]);
        assert_eq!(made, "00000000-0000-4000-8000-000000000000");
        assert_eq!(made.len(), 36);
    }

    #[test]
    fn sizes_and_counts_read_plainly() {
        assert_eq!(size_text(512), "512 bytes");
        assert_eq!(size_text(2048), "2 KB");
        assert_eq!(size_text(340 * 1024 * 1024), "340 MB");
        assert_eq!(size_text(1288 * 1024 * 1024), "1.3 GB");
        assert_eq!(items_text(1), "1 item");
        assert_eq!(bin_text(12, 2048), "12 items, 2 KB");
    }

    #[test]
    fn a_preview_is_one_short_line() {
        assert_eq!(preview("Hello\r\n  world", 40), "Hello world");
        assert_eq!(preview("abcdefghij", 6), "abcde\u{2026}");
        assert_eq!(preview("", 6), "");
    }

    #[test]
    fn copy_ip_copies_the_first_address_or_says_there_is_none() {
        let mut fake = Fake::default();
        assert_eq!(
            run_tool(Tool::CopyIp, &mut fake).text,
            "No network address found"
        );
        fake.addresses = vec![
            Address {
                kind: Kind::WiFi,
                adapter: "Wi-Fi".to_string(),
                ip: "192.168.1.20".to_string(),
            },
            Address {
                kind: Kind::Wsl,
                adapter: "vEthernet (WSL)".to_string(),
                ip: "172.28.160.1".to_string(),
            },
        ];
        let done = run_tool(Tool::CopyIp, &mut fake);
        assert_eq!(done.text, "Copied 192.168.1.20");
        assert_eq!(done.detail.as_deref(), Some("This PC's Wi-Fi address"));
        assert_eq!(fake.clipboard.as_deref(), Some("192.168.1.20"));
        assert!(done.undo.is_none());
    }

    #[test]
    fn dns_and_environment_variables_go_to_windows() {
        let mut fake = Fake::default();
        assert_eq!(
            run_tool(Tool::FlushDns, &mut fake).text,
            "DNS cache cleared"
        );
        assert!(
            run(Tool::EnvVars, &mut fake, &mut None).is_none(),
            "no notice"
        );
        assert_eq!(fake.calls(), ["flush dns", "environment variables"]);
    }

    #[test]
    fn plain_text_puts_the_same_text_back_alone() {
        let mut fake = Fake::default();
        assert_eq!(
            run_tool(Tool::PlainText, &mut fake).text,
            "The clipboard has no text"
        );
        fake.clipboard = Some("Bold words".to_string());
        let done = run_tool(Tool::PlainText, &mut fake);
        assert_eq!(done.text, "The clipboard holds plain text now");
        assert_eq!(fake.calls(), ["clipboard Bold words"]);
    }

    #[test]
    fn the_uuid_the_inspector_showed_is_the_one_copied_and_the_next_is_new() {
        let fake_bytes = uuid_v4(*b"0123456789abcdef");
        let mut fake = Fake::default();
        let mut shown = None;
        let preview = next_uuid(&fake, &mut shown).expect("a uuid");
        assert_eq!(preview, fake_bytes);
        let done = run(Tool::NewUuid, &mut fake, &mut shown).expect("an outcome");
        assert_eq!(done.text, "Copied a new UUID");
        assert_eq!(done.detail.as_deref(), Some(preview.as_str()));
        assert_eq!(fake.clipboard.as_deref(), Some(preview.as_str()));
        assert!(shown.is_none());
    }

    #[test]
    fn the_microphone_mutes_and_undo_turns_it_back_on() {
        let mut fake = Fake::default();
        let done = run_tool(Tool::Microphone, &mut fake);
        assert_eq!(done.text, "Microphone muted");
        assert_eq!(done.detail.as_deref(), Some("Headset"));
        assert_eq!(fake.microphone, Some(true));
        assert_eq!(done.undo, Some(Undo::Microphone(false)));
        assert_eq!(run_tool(Tool::Microphone, &mut fake).text, "Microphone on");
        let mut none = Fake {
            microphone: None,
            ..Fake::default()
        };
        let done = run_tool(Tool::Microphone, &mut none);
        assert_eq!(done.text, "The microphone did not change");
        assert!(none.log.is_empty());
    }

    #[test]
    fn the_recycle_bin_is_emptied_only_when_it_holds_something() {
        let mut fake = Fake::default();
        assert_eq!(
            run_tool(Tool::EmptyBin, &mut fake).text,
            "The Recycle Bin is already empty"
        );
        assert!(fake.log.is_empty());
        fake.bin = Some((12, 340 * 1024 * 1024));
        let done = run_tool(Tool::EmptyBin, &mut fake);
        assert_eq!(done.text, "Recycle Bin emptied");
        assert_eq!(done.detail.as_deref(), Some("12 items, 340 MB"));
        assert!(done.undo.is_none(), "it cannot be undone");
        fake.bin = None;
        assert_eq!(
            run_tool(Tool::EmptyBin, &mut fake).text,
            "The Recycle Bin did not change"
        );
        assert_eq!(
            bin_finished(true, "1 item, 2 KB").text,
            "Recycle Bin emptied"
        );
        assert_eq!(bin_finished(false, "x").glyph, Glyph::Warning);
    }
}
