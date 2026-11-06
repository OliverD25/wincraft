use super::*;

const PLAIN: Layered = Layered {
    layered: false,
    alpha: None,
    color_key: false,
};

fn layered(alpha: Option<u8>) -> Layered {
    Layered {
        layered: true,
        alpha,
        color_key: false,
    }
}

#[test]
fn the_marker_keeps_what_was_there() {
    for saved in [
        Saved {
            added_layered: true,
            alpha: 255,
        },
        Saved {
            added_layered: false,
            alpha: 0,
        },
        Saved {
            added_layered: false,
            alpha: 200,
        },
    ] {
        assert_eq!(Saved::decode(saved.encode(4242)), Some(saved));
        assert_eq!(owner(saved.encode(4242)), 4242);
        assert_eq!(
            Saved::decode(saved.encode(0)),
            Some(saved),
            "a marker of an older WinCraft"
        );
        assert_eq!(owner(saved.encode(0)), 0);
    }
    assert_eq!(Saved::decode(0), None, "no property");
    assert_eq!(Saved::decode(0x1234_0101), None, "someone else's value");
}

#[test]
fn a_plain_popup_gets_the_layered_style_and_loses_it_again() {
    assert_eq!(
        plan(PLAIN),
        Ok(Saved {
            added_layered: true,
            alpha: 255
        })
    );
    assert_eq!(
        plan(layered(Some(230))),
        Ok(Saved {
            added_layered: false,
            alpha: 230
        }),
        "an already layered popup keeps its style and gets its alpha back"
    );
}

#[test]
fn a_popup_that_draws_itself_is_left_alone() {
    assert!(plan(layered(None)).is_err(), "UpdateLayeredWindow");
    let keyed = Layered {
        color_key: true,
        ..layered(Some(255))
    };
    assert!(plan(keyed).is_err(), "colour key");
}

#[test]
fn repair_restores_only_our_own_see_through_popup() {
    let ours = Saved {
        added_layered: true,
        alpha: 255,
    };
    assert_eq!(
        repair_plan(layered(Some(0)), Some(ours), false),
        Repair::Restore(ours)
    );
    assert_eq!(
        repair_plan(layered(Some(0)), None, false),
        Repair::Nothing,
        "alpha 0 without our marker is not ours"
    );
    assert_eq!(repair_plan(PLAIN, None, false), Repair::Nothing);
    assert_eq!(
        repair_plan(layered(Some(255)), Some(ours), false),
        Repair::DropMarker,
        "visible again: only the stale marker goes"
    );
    assert_eq!(repair_plan(PLAIN, Some(ours), false), Repair::DropMarker);
    assert_eq!(
        repair_plan(layered(Some(0)), Some(ours), true),
        Repair::Nothing,
        "another WinCraft that still runs keeps its see-through popup"
    );
}

#[test]
fn only_another_running_wincraft_holds_a_popup() {
    let running = |pid| pid == 7;
    assert!(
        held_elsewhere(7, 1, running),
        "a test copy sees the real one"
    );
    assert!(!held_elsewhere(1, 1, running), "our own marker");
    assert!(!held_elsewhere(9, 1, running), "a WinCraft that crashed");
    assert!(
        !held_elsewhere(0, 1, |_| true),
        "an older WinCraft's marker"
    );
}

/// The proof that see-through works on a window of another process. It
/// starts the demo `Notes.exe` that `tools\scene-shot.ps1 -Demo` builds (a
/// program of our own, never a user window), makes its backdrop
/// see-through from this process, reads it back, restores it, and closes it.
/// It shows a full-screen backdrop for about a second. Run by hand:
/// `cargo test see_through_works_on_another_process -- --ignored --nocapture`
#[test]
#[ignore = "starts the demo Notes.exe and shows it on screen; run by hand"]
fn see_through_works_on_another_process() {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::LPARAM;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId,
    };

    let local = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA");
    let built = PathBuf::from(&local).join(r"Temp\wincraft-scene-demo\Notes.exe");
    assert!(
        built.exists(),
        "run tools\\scene-shot.ps1 -Demo once to build {}",
        built.display()
    );
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let dir = std::env::temp_dir().join(format!("wincraft-see-through-proof-{stamp}"));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("Notes.exe");
    std::fs::copy(&built, &exe).unwrap();
    let mut child = std::process::Command::new(&exe)
        .arg(&dir)
        .arg("light")
        .arg("none")
        .spawn()
        .expect("start Notes.exe");
    let quit = |child: &mut std::process::Child| {
        let _ = std::fs::write(dir.join("quit.txt"), "quit");
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    while !dir.join("ready.txt").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }

    struct Find {
        pid: u32,
        found: HWND,
    }
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> windows_sys::core::BOOL {
        let find = unsafe { &mut *(data as *mut Find) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        let mut text = [0u16; 64];
        let len = unsafe { GetWindowTextW(hwnd, text.as_mut_ptr(), 64) };
        if pid == find.pid && String::from_utf16_lossy(&text[..len as usize]) == "Desktop notes" {
            find.found = hwnd;
            return 0;
        }
        1
    }
    let mut find = Find {
        pid: child.id(),
        found: std::ptr::null_mut(),
    };
    unsafe { EnumWindows(Some(each), &mut find as *mut Find as LPARAM) };
    let hwnd = find.found;
    if hwnd.is_null() {
        quit(&mut child);
        panic!("the demo window did not appear");
    }

    let before = read(hwnd);
    let style_before = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    let applied = apply(hwnd);
    let during = read(hwnd);
    let marked = marker(hwnd);
    let restored = restore(hwnd);
    let after = read(hwnd);
    let style_after = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    let marker_after = marker(hwnd);
    // A crash between apply and restore: repair at the next start.
    let applied_again = apply(hwnd);
    let repaired = repair(hwnd);
    let after_repair = read(hwnd);
    quit(&mut child);

    println!("before {before:?} style {style_before:#x}");
    println!("apply {applied:?}, during {during:?}, marker {marked:?}");
    println!("restore {restored}, after {after:?} style {style_after:#x}, marker {marker_after:?}");
    println!("apply again {applied_again:?}, repair {repaired:?}, after repair {after_repair:?}");
    assert_eq!(applied, Ok(true));
    assert_eq!(during, layered(Some(0)));
    assert_eq!(
        marked,
        Some(Saved {
            added_layered: !before.layered,
            alpha: 255
        })
    );
    assert!(restored);
    assert_eq!(after, before);
    assert_eq!(style_after, style_before);
    assert_eq!(marker_after, None);
    assert_eq!(applied_again, Ok(true));
    assert!(matches!(repaired, Repair::Restore(_)));
    assert_eq!(after_repair, before);
}

/// Only reads Explorer's real taskbar popups and prints their state, so a
/// person can see whether see-through can apply there. Changes nothing.
#[test]
#[ignore = "reads Explorer's real taskbar popups; run by hand"]
fn print_the_real_popups_state() {
    for hwnd in super::super::find_popups() {
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
        println!(
            "popup {:#x}: ex-style {style:#x}, {:?}, marker {:?}, plan {:?}",
            hwnd as usize,
            read(hwnd),
            marker(hwnd),
            plan(read(hwnd))
        );
    }
}
