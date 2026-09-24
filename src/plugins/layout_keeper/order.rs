use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::CLSCTX_INPROC_SERVER;
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use super::com::{self, ComPtr};
use super::identity::{match_windows, WindowIdentity};

/// A window handle kept as a number: the model is compared and cloned freely,
/// and a raw HWND is neither Eq nor Send.
pub type Handle = isize;

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    identity: WindowIdentity,
    hwnd: Option<Handle>,
    /// Tick at which the window was last missing from a refresh, if it is.
    missing_since: Option<u64>,
}

/// The wanted thumbnail order of one program's taskbar group. Windows keeps
/// no readable record of that order, so this model is the truth and the
/// taskbar is made to match it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrderModel {
    entries: Vec<Entry>,
    applied: Option<Vec<Handle>>,
}

impl OrderModel {
    /// A model remembered from an earlier session. Its windows have no
    /// handles until a refresh recognises them.
    pub fn from_saved(identities: Vec<WindowIdentity>) -> Self {
        Self {
            entries: identities
                .into_iter()
                .map(|identity| Entry {
                    identity,
                    hwnd: None,
                    missing_since: None,
                })
                .collect(),
            applied: None,
        }
    }

    /// Brings the model up to date with the live windows, listed top of the
    /// z-order first. Windows already known by handle stay put, others are
    /// recognised by identity, and the rest join at the end bottom first, the
    /// way Windows appends a new button. A window missing for `drop_after`
    /// ticks is forgotten.
    pub fn refresh(&mut self, live: &[(Handle, WindowIdentity)], tick: u64, drop_after: u64) {
        let mut live_used = vec![false; live.len()];
        let mut entry_found = vec![false; self.entries.len()];

        for (e_index, entry) in self.entries.iter_mut().enumerate() {
            let Some(hwnd) = entry.hwnd else { continue };
            if let Some(l_index) = live.iter().position(|(h, _)| *h == hwnd) {
                entry.identity = live[l_index].1.clone();
                live_used[l_index] = true;
                entry_found[e_index] = true;
            }
        }

        let waiting: Vec<usize> = (0..self.entries.len())
            .filter(|i| !entry_found[*i])
            .collect();
        let free: Vec<usize> = (0..live.len()).filter(|i| !live_used[*i]).collect();
        let saved: Vec<WindowIdentity> = waiting
            .iter()
            .map(|i| self.entries[*i].identity.clone())
            .collect();
        let candidates: Vec<WindowIdentity> = free.iter().map(|i| live[*i].1.clone()).collect();
        for (s, l) in match_windows(&saved, &candidates).pairs {
            let (e_index, l_index) = (waiting[s], free[l]);
            let entry = &mut self.entries[e_index];
            entry.identity = live[l_index].1.clone();
            entry.hwnd = Some(live[l_index].0);
            live_used[l_index] = true;
            entry_found[e_index] = true;
        }

        for (entry, found) in self.entries.iter_mut().zip(&entry_found) {
            if *found {
                entry.missing_since = None;
            } else {
                entry.hwnd = None;
                entry.missing_since.get_or_insert(tick);
            }
        }
        self.entries.retain(|entry| {
            entry
                .missing_since
                .is_none_or(|since| tick.saturating_sub(since) < drop_after)
        });

        for l_index in (0..live.len()).rev().filter(|i| !live_used[*i]) {
            self.entries.push(Entry {
                identity: live[l_index].1.clone(),
                hwnd: Some(live[l_index].0),
                missing_since: None,
            });
        }
    }

    /// The live windows in thumbnail order.
    pub fn handles(&self) -> Vec<Handle> {
        self.entries.iter().filter_map(|entry| entry.hwnd).collect()
    }

    pub fn position_of(&self, hwnd: Handle) -> Option<usize> {
        self.handles().iter().position(|h| *h == hwnd)
    }

    /// Swaps a window with its live neighbour, `-1` towards the start of the
    /// group. Returns false at either end or for an unknown window.
    pub fn shift(&mut self, hwnd: Handle, step: isize) -> bool {
        let live: Vec<usize> = (0..self.entries.len())
            .filter(|i| self.entries[*i].hwnd.is_some())
            .collect();
        let Some(k) = live
            .iter()
            .position(|i| self.entries[*i].hwnd == Some(hwnd))
        else {
            return false;
        };
        let Some(other) = k.checked_add_signed(step).and_then(|j| live.get(j)) else {
            return false;
        };
        self.entries.swap(live[k], *other);
        true
    }

    /// What `apply` would send, or None when the taskbar already shows it.
    pub fn pending(&self) -> Option<Vec<Handle>> {
        let handles = self.handles();
        (self.applied.as_ref() != Some(&handles) && !handles.is_empty()).then_some(handles)
    }

    pub fn mark_applied(&mut self, handles: Vec<Handle>) {
        self.applied = Some(handles);
    }
}

const CLSID_TASKBAR_LIST: GUID = GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090);
const IID_ITASKBAR_LIST: GUID = GUID::from_u128(0x56fdf342_fd6d_11d0_958a_006097c9a090);

#[repr(C)]
struct ITaskbarListVtbl {
    _base: IUnknown_Vtbl,
    hr_init: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    add_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    delete_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    activate_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    _set_active_alt: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
}

/// The public taskbar interface. Deleting a window's button and adding it
/// again puts its thumbnail last in its group, for any process's window, so
/// re-adding a whole group in model order produces exactly that order.
pub struct Taskbar(ComPtr);

impl Taskbar {
    pub fn new() -> Result<Self, String> {
        let list = ComPtr::create(
            &CLSID_TASKBAR_LIST,
            &IID_ITASKBAR_LIST,
            CLSCTX_INPROC_SERVER,
        )
        .map_err(|hr| format!("ITaskbarList unavailable ({})", com::hex(hr)))?;
        let hr = unsafe { (list.vtable::<ITaskbarListVtbl>().hr_init)(list.as_raw()) };
        com::check(hr).map_err(|hr| format!("ITaskbarList::HrInit failed ({})", com::hex(hr)))?;
        Ok(Self(list))
    }

    /// Re-adds the windows in order, then hands the highlight back to the
    /// window that had it, so the apply is not visible as a focus change.
    pub fn apply(&self, handles: &[Handle]) -> Duration {
        let started = Instant::now();
        let vtable = unsafe { self.0.vtable::<ITaskbarListVtbl>() };
        let foreground = unsafe { GetForegroundWindow() } as Handle;
        for hwnd in handles {
            unsafe {
                (vtable.delete_tab)(self.0.as_raw(), *hwnd as HWND);
                (vtable.add_tab)(self.0.as_raw(), *hwnd as HWND);
            }
        }
        if handles.contains(&foreground) {
            unsafe { (vtable.activate_tab)(self.0.as_raw(), foreground as HWND) };
        }
        started.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(title: &str) -> WindowIdentity {
        WindowIdentity::new("chrome.exe", title, [-11, -11, 3851, 2099], true)
    }

    /// Live windows top of the z-order first, handle = 100 + position.
    fn live(titles: &[&str]) -> Vec<(Handle, WindowIdentity)> {
        titles
            .iter()
            .enumerate()
            .map(|(i, t)| (100 + i as Handle, window(t)))
            .collect()
    }

    fn labels(model: &OrderModel) -> Vec<String> {
        model
            .entries
            .iter()
            .filter(|entry| entry.hwnd.is_some())
            .map(|entry| entry.identity.title.clone())
            .collect()
    }

    #[test]
    fn a_group_seen_for_the_first_time_is_ordered_bottom_first() {
        let mut model = OrderModel::default();
        model.refresh(&live(&["Top", "Middle", "Bottom"]), 0, 30);
        assert_eq!(labels(&model), ["Bottom", "Middle", "Top"]);
    }

    #[test]
    fn a_saved_model_keeps_its_order_and_new_windows_are_appended() {
        let mut model = OrderModel::from_saved(vec![window("B"), window("A")]);
        model.refresh(&live(&["New", "A", "B"]), 0, 30);
        assert_eq!(labels(&model), ["B", "A", "New"]);
    }

    #[test]
    fn a_missing_window_is_dropped_only_after_the_interval() {
        let mut model = OrderModel::default();
        model.refresh(&live(&["A", "B"]), 0, 30);
        let only_a = vec![(100, window("A"))];
        model.refresh(&only_a, 10, 30);
        assert_eq!(model.entries.len(), 2);
        assert_eq!(model.handles(), [100]);
        model.refresh(&only_a, 40, 30);
        assert_eq!(model.entries.len(), 1);
    }

    #[test]
    fn a_window_that_comes_back_takes_its_old_place() {
        let mut model = OrderModel::from_saved(vec![window("A"), window("B"), window("C")]);
        model.refresh(&live(&["C", "B", "A"]), 0, 30);
        // B closes and reopens with a new handle.
        let without_b = vec![(100, window("C")), (102, window("A"))];
        model.refresh(&without_b, 5, 30);
        let back = vec![(100, window("C")), (200, window("B")), (102, window("A"))];
        model.refresh(&back, 10, 30);
        assert_eq!(labels(&model), ["A", "B", "C"]);
        assert_eq!(model.position_of(200), Some(1));
    }

    #[test]
    fn shifting_stops_at_the_edges() {
        let mut model = OrderModel::default();
        model.refresh(&live(&["C", "B", "A"]), 0, 30);
        // Order is A(102), B(101), C(100).
        assert!(!model.shift(102, -1));
        assert!(!model.shift(100, 1));
        assert!(model.shift(102, 1));
        assert_eq!(labels(&model), ["B", "A", "C"]);
        assert!(model.shift(100, -1));
        assert_eq!(labels(&model), ["B", "C", "A"]);
        assert!(!model.shift(999, 1));
    }

    #[test]
    fn apply_is_needed_only_when_the_order_changed() {
        let mut model = OrderModel::default();
        model.refresh(&live(&["B", "A"]), 0, 30);
        let pending = model.pending().expect("never applied");
        model.mark_applied(pending);
        assert_eq!(model.pending(), None);
        model.refresh(&live(&["B", "A"]), 1, 30);
        assert_eq!(model.pending(), None);
        assert!(model.shift(101, 1));
        assert_eq!(model.pending(), Some(vec![100, 101]));
    }
}
