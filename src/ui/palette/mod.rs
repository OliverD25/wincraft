mod actions;
mod dispatch;
mod drag;
mod inspector;
mod panel;
mod pinned;
mod prompt;
mod rows;
mod state;
mod view;
pub use drag::Step as WindowStep;
use panel::*;
#[cfg(test)]
use prompt::prompt_row_title;
use rows::*;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::LayoutJob;
use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, Sense, StrokeKind,
    UiBuilder,
};
use windows_sys::Win32::Foundation::HWND;

use crate::core::theme::{self, Tokens};
use crate::core::tips::{self, Place, TipsConfig};
use crate::core::ui_bridge::{
    CommandDetails, CommandId, HostChannel, HostRequest, HostSetting, ListRow, Page, Prompt,
    SubList, ToggleLook, ToggleState, UiSnapshot, SEARCH_PROMPT,
};
use crate::core::virtual_desktops::Pinner;
use crate::core::window_info::{self, WindowDetails};
use crate::search::bookmarks as bookmark_store;
use crate::search::icons::IconCache;
use crate::search::providers::bookmarks as bookmark_rows;
use crate::search::{
    Action, Ask, Choice, Context, Glyph, IconRef, Reply, ResultItem, Results, Router, Tone,
    SUGGESTED_GROUP,
};
use crate::ui::dwm_thumbs::{self, Placement, Thumbnail};
use crate::ui::fuzzy;
use crate::ui::widgets::hotkey_dialog::{self, Editor, Target};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::tip::{self, STRIP_HEIGHT};
use crate::ui::widgets::{keycap, row, text, toggle};
use actions::{Hint, Key, Marks, RowAction, Run, Situation};
use inspector::Inspected;
pub use pinned::set_scene_select_update;
use pinned::{PinnedUpdate, UPDATE_GROUP};

pub use actions::is_narrow;

/// The panel the user sees. On a monitor narrower than 1100 px the panel
/// is the list alone and the inspector opens over it.
pub const PANEL_WIDTH: f32 = 900.0;
pub const NARROW_PANEL_WIDTH: f32 = PANEL_WIDTH - inspector::WIDTH;
pub const PANEL_HEIGHT: f32 = 520.0;
/// The shadow is drawn inside the window, so the OS window is the panel plus a
/// 24 px transparent margin on every side.
pub const MARGIN: f32 = 24.0;
pub const WIDTH: f32 = PANEL_WIDTH + MARGIN * 2.0;
pub const HEIGHT: f32 = PANEL_HEIGHT + MARGIN * 2.0;

const SEARCH_HEIGHT: f32 = 56.0;
const FOOTER_HEIGHT: f32 = 40.0;
const COMPACT_HEIGHT: f32 = 24.0;
const ROW_PADDING: f32 = 12.0;
const ROW_MIN_HEIGHT: f32 = 44.0;
const ICON_SLOT: f32 = 26.0;
/// A toggle's state tile; it sits centred on the icon slot.
const STATE_TILE: f32 = 28.0;
const CHECK_SIZE: f32 = 16.0;
const GROUP_HEADER: f32 = 28.0;
const TITLE_LINE: f32 = 20.0;
const ANSWER_LINE: f32 = 26.0;
const SUBTITLE_LINE: f32 = 16.0;
const RISE: f32 = 4.0;
/// How long the palette stays open after a bookmark was saved, so the
/// confirmation can be read.
const SAVED_FOR: Duration = Duration::from_secs(4);

pub enum Outcome {
    Stay,
    Hide,
    OpenPlugin(String),
    /// Settings → Hotkeys on a command that the snapshot does not list:
    /// its id and name.
    OpenShortcut {
        id: String,
        label: String,
    },
    /// The footer's gear, Ctrl+, or Ctrl+P.
    OpenSettings(Page),
}

pub struct Palette {
    /// Windows' glass shows behind the panel: the window is the panel, with
    /// no room around it for a shadow of its own.
    glass: bool,
    query: String,
    /// The prefix the user typed, drawn as a chip before the search box. The
    /// query holds only what came after it.
    prefix: Option<String>,
    router: Router,
    results: Results,
    /// The prefix and query `results` answer; None when they are out of date.
    searched: Option<(Option<String>, String)>,
    selected: usize,
    focused_once: bool,
    follow_selection: bool,
    /// The query was changed in code, so the text cursor goes to its end.
    cursor_to_end: bool,
    /// Made on the first frame, because it needs the egui context.
    icons: Option<IconCache>,
    /// A plugin's question, such as a window's new name. While it is set the
    /// palette is a one-line text box that answers it, not a search.
    prompt: Option<Prompt>,
    /// The prompt's text was put in the box and is selected once, so typing
    /// replaces it.
    select_all: bool,
    /// The host's tip settings and state, refreshed from each snapshot and
    /// sent back whole after this palette changes them.
    tips: TipsConfig,
    tips_synced: bool,
    /// This opening already counted as one show of the folder tip.
    tip_counted: bool,
    /// A bookmark was just saved here: shown as a confirmation.
    saved: Option<Saved>,
    /// Made when first needed, so a palette that never looks at bookmarks
    /// never reads the file.
    bookmarks: Option<bookmark_store::Shared>,
    /// Whether the place the selected row opens is a folder, looked up once.
    place: Option<(String, Place)>,
    /// The monitor it opened on is narrower than 1100 px.
    narrow: bool,
    /// On a narrow monitor, the inspector is open over the list.
    overlay: bool,
    /// After →, the keyboard is in the inspector's action list, here.
    action_cursor: Option<usize>,
    marks: Marks,
    /// The results are the "Try instead" rows, since nothing matched.
    trying: bool,
    /// The palette's own window, which the live window picture is drawn on.
    window: isize,
    thumbnail: Option<(isize, Thumbnail)>,
    /// What the inspector shows about the selected window, read once when
    /// the cursor reaches it.
    details: Option<(isize, WindowDetails)>,
    /// What the selected plugin command's plugin said about it, asked once
    /// when the cursor gets there; None inside until the answer comes.
    command_details: Option<(CommandId, Option<CommandDetails>)>,
    /// The toggle commands' state, read by their plugins as the palette
    /// opened and after each flip; empty until the host answers.
    toggle_states: Vec<(CommandId, ToggleState)>,
    /// A command, or a row of its sub-list (the row's id; empty for the
    /// command itself), that asks for Enter twice got its first, then.
    armed: Option<(CommandId, String, Instant)>,
    /// A command's sub-list shown in place of the results.
    sub_list: Option<SubListView>,
    /// Made when first needed; None inside when the shell's interface for
    /// pinning is not there.
    pinner: Option<Option<Pinner>>,
    /// The search box's id from the last frame, to read its text cursor
    /// before it is drawn again.
    query_id: Option<egui::Id>,
    /// Ctrl+K's hotkey dialog, over the panel.
    hotkey_dialog: Option<Editor>,
    /// Where the panel was drawn last, for the dialog's backdrop.
    panel: Rect,
    /// The search box's empty part, right of the text: blank space to drag.
    search_blank: Rect,
    blank: drag::BlankPress,
    window_step: drag::Step,
    /// The "Update ready" row as the snapshot had it when the palette
    /// opened; None inside when there is none. Kept for the whole opening,
    /// so the list never changes under the user's eyes.
    pinned: Option<Option<PinnedUpdate>>,
    /// The cursor was put back to the top (an opening, a new query), so the
    /// next list with the pinned row starts on the row under it.
    pin_fresh: bool,
}

/// A command's rows, shown in place of the search results until Enter
/// picks one or Esc goes back.
pub(super) struct SubListView {
    pub(super) id: CommandId,
    /// None until the host's answer comes.
    pub(super) list: Option<SubList>,
    /// The prefix and the text to put back on Esc.
    pub(super) back: (Option<String>, String),
}

/// The line over the footer.
enum Strip {
    Hint(Place),
    Saved(String),
}

struct Saved {
    phrase: String,
    /// When the palette closes by itself; None keeps it open, as a test
    /// scene needs.
    until: Option<Instant>,
}

/// A test scene started with `--show-tip=bookmark-saved` opens on the
/// confirmation, since a screenshot sends no Enter to save one.
fn scene_saved(scene: Option<&str>) -> Option<Saved> {
    (scene == Some(tips::BOOKMARK_SAVED.id)).then(|| Saved {
        phrase: "pswds".to_string(),
        until: None,
    })
}

/// Where a run came from, so only a row's own Enter or click counts as a
/// pick for the ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Via {
    Pick,
    Other,
}

impl Palette {
    pub fn new(router: Router) -> Self {
        Self {
            query: String::new(),
            prefix: None,
            router,
            results: Results::default(),
            searched: None,
            selected: 0,
            focused_once: false,
            follow_selection: false,
            cursor_to_end: false,
            icons: None,
            prompt: None,
            select_all: false,
            tips: TipsConfig::default(),
            tips_synced: false,
            tip_counted: false,
            saved: None,
            bookmarks: None,
            place: None,
            narrow: false,
            glass: false,
            overlay: false,
            action_cursor: None,
            marks: Marks::default(),
            trying: false,
            window: 0,
            thumbnail: None,
            details: None,
            command_details: None,
            toggle_states: Vec::new(),
            armed: None,
            sub_list: None,
            pinner: None,
            query_id: None,
            hotkey_dialog: None,
            panel: Rect::NOTHING,
            search_blank: Rect::NOTHING,
            blank: Default::default(),
            window_step: Default::default(),
            pinned: None,
            pin_fresh: false,
        }
    }

    /// The palette's own window, which the live picture of the selected
    /// window is drawn on.
    pub fn set_window(&mut self, hwnd: isize) {
        self.window = hwnd;
    }

    pub fn opened(&mut self) {
        self.prompt = None;
        self.select_all = false;
        self.tip_counted = false;
        self.saved = scene_saved(tips::scene_tip().as_deref());
        self.query.clear();
        self.prefix = None;
        self.selected = 0;
        self.focused_once = false;
        self.follow_selection = true;
        self.router.opened();
        self.searched = None;
        self.overlay = false;
        self.action_cursor = None;
        self.marks.clear();
        self.details = None;
        self.command_details = None;
        self.toggle_states.clear();
        self.armed = None;
        self.sub_list = None;
        self.thumbnail = None;
        self.pinned = None;
        self.pin_fresh = true;
        if let Some(icons) = &mut self.icons {
            icons.forget_windows();
        }
    }

    /// The palette's window was hidden: the live picture goes with it, and
    /// an open hotkey dialog with its keyboard hook.
    pub fn hidden(&mut self) {
        self.thumbnail = None;
        self.hotkey_dialog = None;
    }

    /// On a monitor narrower than 1100 px the panel is the list alone.
    pub fn set_narrow(&mut self, narrow: bool) {
        self.narrow = narrow;
    }

    pub fn panel_width(&self) -> f32 {
        if self.narrow {
            NARROW_PANEL_WIDTH
        } else {
            PANEL_WIDTH
        }
    }

    /// The OS window's width: the panel and the shadow margins.
    pub fn window_width(&self) -> f32 {
        self.panel_width() + self.margin() * 2.0
    }

    pub fn window_height(&self) -> f32 {
        PANEL_HEIGHT + self.margin() * 2.0
    }

    /// The transparent room for the panel's shadow; none with the glass,
    /// where the window is the panel.
    pub fn margin(&self) -> f32 {
        if self.glass {
            0.0
        } else {
            MARGIN
        }
    }

    pub fn glass(&self) -> bool {
        self.glass
    }

    pub fn set_glass(&mut self, on: bool) {
        self.glass = on;
    }

    /// Turns the freshly opened palette into a text box for `prompt`.
    pub fn ask(&mut self, prompt: Prompt) {
        self.query = prompt.text.clone();
        self.select_all = true;
        self.prompt = Some(prompt);
    }
}

#[cfg(test)]
mod tests;
