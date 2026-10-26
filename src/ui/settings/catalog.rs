//! What the Plugins page and the Plugin Store share: the plugin cards, the
//! search and category filter, the tile grid and its keyboard handling.

use egui::{pos2, vec2, Align, EventFilter, Id, Key, Layout, Modifiers, Rect, Sense, Ui};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::PluginInfo;
use crate::store::github;
use crate::store::index::IndexEntry;
use crate::ui::widgets::button::Kind;
use crate::ui::widgets::icons::Icon;
use crate::ui::widgets::tile::{self, Tile};
use crate::ui::widgets::{chips, choice, focus, search_box, text};

/// The group a plugin without a category falls into.
pub const OTHER: &str = "Other";

/// The facts a tile shows, whether they came from an installed plugin or from
/// the store's index.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub category: String,
}

fn category_name(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        OTHER.to_string()
    } else {
        raw.to_string()
    }
}

impl Card {
    pub fn from_plugin(plugin: &PluginInfo) -> Self {
        Self {
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            author: plugin.author.clone(),
            description: plugin.description.clone(),
            category: category_name(&plugin.category),
        }
    }

    /// An index written before categories existed has none; the installed
    /// copy of the same plugin knows its own.
    pub fn from_entry(entry: &IndexEntry, installed: Option<&PluginInfo>) -> Self {
        let category = if entry.category.trim().is_empty() {
            installed
                .map(|plugin| plugin.category.as_str())
                .unwrap_or("")
        } else {
            entry.category.as_str()
        };
        Self {
            id: entry.id.clone(),
            name: entry.name.clone(),
            author: entry.author.clone(),
            description: entry.description.clone(),
            category: category_name(category),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// The order the plugins come in.
    #[default]
    Listed,
    Name,
    Category,
}

impl Sort {
    pub const CHOICES: [Sort; 2] = [Sort::Name, Sort::Category];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Listed => "Listed",
            Sort::Name => "Name",
            Sort::Category => "Category",
        }
    }
}

/// What the search box, the category chips and the sort menu are set to.
#[derive(Default)]
pub struct View {
    pub query: String,
    /// `None` is the "All" chip.
    pub category: Option<String>,
    pub sort: Sort,
}

impl View {
    pub fn sorted_by(sort: Sort) -> Self {
        Self {
            sort,
            ..Self::default()
        }
    }
}

/// Every word typed must appear in the name, the description, the category or
/// the author, in any case.
pub fn matches(card: &Card, query: &str) -> bool {
    let text = format!(
        "{} {} {} {}",
        card.name, card.description, card.category, card.author
    )
    .to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}

/// The cards to show, as positions in `cards`, in the view's order.
pub fn visible(cards: &[Card], view: &View) -> Vec<usize> {
    let mut order: Vec<usize> = (0..cards.len())
        .filter(|&at| {
            let card = &cards[at];
            matches(card, &view.query)
                && view
                    .category
                    .as_ref()
                    .is_none_or(|category| *category == card.category)
        })
        .collect();
    let name = |at: &usize| cards[*at].name.to_lowercase();
    match view.sort {
        Sort::Listed => {}
        Sort::Name => order.sort_by_key(name),
        Sort::Category => order.sort_by_key(|at| (cards[*at].category.to_lowercase(), name(at))),
    }
    order
}

/// The categories in the order they first appear, each with how many cards
/// match the search. A chip never promises tiles the grid will not show.
pub fn counts(cards: &[Card], query: &str) -> Vec<(String, usize)> {
    let mut found: Vec<(String, usize)> = Vec::new();
    for card in cards {
        let hit = usize::from(matches(card, query));
        match found.iter_mut().find(|(name, _)| *name == card.category) {
            Some((_, count)) => *count += hit,
            None => found.push((card.category.clone(), hit)),
        }
    }
    found
}

/// A plugin's picture when it has no thumbnail: the icon of its category.
pub fn icon_for(category: &str) -> Icon {
    match category {
        "Monitors" => Icon::Monitor,
        "Keyboard" => Icon::Keyboard,
        "Windows" => Icon::LayoutGrid,
        "Input" => Icon::Globe,
        _ => Icon::Terminal,
    }
}

pub fn count_in_words(count: usize) -> String {
    const WORDS: [&str; 11] = [
        "No plugins",
        "One",
        "Two",
        "Three",
        "Four",
        "Five",
        "Six",
        "Seven",
        "Eight",
        "Nine",
        "Ten",
    ];
    WORDS
        .get(count)
        .map(|word| word.to_string())
        .unwrap_or_else(|| count.to_string())
}

// ---------------------------------------------------------------- store state

/// How a store tile relates to this copy of WinCraft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreState {
    /// Installed and switched on.
    Enabled,
    /// Installed and switched off.
    Disabled,
    /// Only a newer WinCraft has it; holds the version that does.
    NeedsUpdate(String),
    /// Not installed, and this WinCraft is new enough to take it.
    NotInstalled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Good,
    Quiet,
    Warn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadgeSpec {
    pub label: String,
    pub tone: Tone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreAction {
    /// Switch the installed plugin on.
    Enable,
    /// Plugins come with WinCraft, so this opens the releases page.
    OpenReleases,
}

/// What a store tile shows at its right end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileControl {
    /// A plain "Installed" mark, not a button.
    Installed,
    Button(ButtonSpec),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonSpec {
    pub label: &'static str,
    pub kind: Kind,
    pub action: StoreAction,
}

impl StoreState {
    /// `installed` holds whether the plugin is switched on, when it is
    /// installed. `listed_in` is the WinCraft version the index belongs to.
    pub fn of(installed: Option<bool>, listed_in: &str, running: &str) -> Self {
        match installed {
            Some(true) => StoreState::Enabled,
            Some(false) => StoreState::Disabled,
            None if github::is_newer(listed_in, running) => {
                StoreState::NeedsUpdate(listed_in.to_string())
            }
            None => StoreState::NotInstalled,
        }
    }

    pub fn badge(&self) -> Option<BadgeSpec> {
        let (label, tone) = match self {
            StoreState::Enabled => ("Enabled".to_string(), Tone::Good),
            StoreState::Disabled => ("Installed, disabled".to_string(), Tone::Quiet),
            StoreState::NeedsUpdate(version) => (format!("Needs WinCraft {version}"), Tone::Warn),
            StoreState::NotInstalled => return None,
        };
        Some(BadgeSpec { label, tone })
    }

    pub fn tile_control(&self) -> TileControl {
        match self {
            StoreState::Enabled | StoreState::Disabled => TileControl::Installed,
            _ => self
                .button()
                .map_or(TileControl::Installed, TileControl::Button),
        }
    }

    pub fn button(&self) -> Option<ButtonSpec> {
        match self {
            StoreState::Enabled => None,
            StoreState::Disabled => Some(ButtonSpec {
                label: "Enable",
                kind: Kind::Primary,
                action: StoreAction::Enable,
            }),
            StoreState::NeedsUpdate(_) => Some(ButtonSpec {
                label: "Get update",
                kind: Kind::Secondary,
                action: StoreAction::OpenReleases,
            }),
            StoreState::NotInstalled => Some(ButtonSpec {
                label: "Install",
                kind: Kind::Primary,
                action: StoreAction::OpenReleases,
            }),
        }
    }
}

// ------------------------------------------------------------------- keyboard

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    First,
    Last,
}

/// Where an arrow key takes the focus among `len` tiles laid out `columns` to
/// a row. It stops at the edges. Down from a row above a shorter last row
/// lands on the last tile instead of doing nothing.
pub fn step(current: usize, to: Move, columns: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let columns = columns.max(1);
    let last = len - 1;
    let current = current.min(last);
    match to {
        Move::Left => current.saturating_sub(1),
        Move::Right => (current + 1).min(last),
        Move::Up if current >= columns => current - columns,
        Move::Up => current,
        Move::Down if current + columns <= last => current + columns,
        Move::Down if last / columns > current / columns => last,
        Move::Down => current,
        Move::First => 0,
        Move::Last => last,
    }
}

/// The arrow, Home or End key pressed this frame. It is taken out of the
/// frame's input, so the side menu does not also use it to change page.
fn take_move(ui: &Ui) -> Option<Move> {
    const KEYS: [(Key, Move); 6] = [
        (Key::ArrowLeft, Move::Left),
        (Key::ArrowRight, Move::Right),
        (Key::ArrowUp, Move::Up),
        (Key::ArrowDown, Move::Down),
        (Key::Home, Move::First),
        (Key::End, Move::Last),
    ];
    ui.input_mut(|input| {
        KEYS.iter()
            .find(|(key, _)| input.consume_key(Modifiers::NONE, *key))
            .map(|(_, to)| *to)
    })
}

// -------------------------------------------------------------------- drawing

/// The page title at 30 px with its line under it. `right` is laid out right
/// to left, so its first widget is rightmost.
pub fn page_title(ui: &mut Ui, title: &str, subtitle: &str, right: impl FnOnce(&mut Ui)) {
    let tokens = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        text::single(
            ui,
            text::job(
                title,
                theme::semibold(30.0),
                tokens.text_primary,
                Some(36.0),
            ),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
    ui.add_space(6.0);
    text::wrapped(
        ui,
        text::job(
            subtitle,
            theme::regular(14.0),
            tokens.text_secondary,
            Some(20.0),
        ),
    );
}

const SORT_WIDTH: f32 = 152.0;
const SEARCH_ROW: f32 = 36.0;

/// The search row and the category chips. Ctrl+F focuses the search box.
pub fn toolbar(ui: &mut Ui, view: &mut View, cards: &[Card], placeholder: &str, sortable: bool) {
    let search_id = ui.id().with("search");
    if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::F)) {
        ui.memory_mut(|memory| memory.request_focus(search_id));
    }

    ui.horizontal(|ui| {
        ui.set_height(SEARCH_ROW);
        let reserved = if sortable { SORT_WIDTH + 16.0 } else { 0.0 };
        let width = (ui.available_width() - reserved).clamp(160.0, 420.0);
        search_box::search_box(ui, search_id, &mut view.query, placeholder, width);
        if sortable {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let labels: Vec<&str> = Sort::CHOICES.iter().map(|sort| sort.label()).collect();
                let selected = Sort::CHOICES.iter().position(|sort| *sort == view.sort);
                let chosen = choice::dropdown_sized(
                    ui,
                    "sort",
                    &labels,
                    selected,
                    "Sort: ",
                    vec2(SORT_WIDTH, SEARCH_ROW),
                );
                if let Some(at) = chosen {
                    view.sort = Sort::CHOICES[at];
                }
            });
        }
    });
    ui.add_space(20.0);

    let by_category = counts(cards, &view.query);
    if view
        .category
        .as_ref()
        .is_some_and(|chosen| !by_category.iter().any(|(name, _)| name == chosen))
    {
        view.category = None;
    }
    let total: usize = by_category.iter().map(|(_, count)| count).sum();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if chips::chip(ui, "All", total, view.category.is_none()).clicked() {
            view.category = None;
        }
        for (name, count) in &by_category {
            let chosen = view.category.as_ref() == Some(name);
            if chips::chip(ui, name, *count, chosen).clicked() {
                view.category = Some(name.clone());
            }
        }
    });
    ui.add_space(20.0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// A click, or Enter on a tile.
    Open,
    /// Space on a tile.
    Toggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Picked {
    /// The position in the `cards` slice given to `tiles`.
    pub card: usize,
    pub how: Activation,
}

fn tile_id(grid: Id, card: &Card) -> Id {
    grid.with(("tile", &card.id))
}

/// The grid of tiles for `order` (positions in `cards`). `control` draws what
/// sits at the right end of a tile. Arrow keys, Home and End move focus
/// between tiles; they are taken from the frame's input while a tile has
/// focus.
pub fn tiles(
    ui: &mut Ui,
    cards: &[Card],
    order: &[usize],
    mut control: impl FnMut(&mut Ui, &Card),
) -> Option<Picked> {
    let width = ui.available_width();
    let columns = tile::columns(width);
    let each = tile::tile_width(width, columns);
    let rows = order.len().div_ceil(columns);
    let height = rows as f32 * tile::HEIGHT + rows.saturating_sub(1) as f32 * tile::GAP;
    let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());

    let grid = ui.id();
    let ids: Vec<Id> = order.iter().map(|&at| tile_id(grid, &cards[at])).collect();
    let mut moved_to = None;
    if let Some(at) = ids
        .iter()
        .position(|id| ui.memory(|memory| memory.has_focus(*id)))
    {
        // Without this egui moves focus to whatever lies in that direction,
        // which would land on a tile's control.
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                ids[at],
                EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..EventFilter::default()
                },
            )
        });
        if let Some(to) = take_move(ui) {
            let target = step(at, to, columns, order.len());
            ui.memory_mut(|memory| memory.request_focus(ids[target]));
            moved_to = Some(target);
        }
    }

    let space = ui.input(|input| input.key_pressed(Key::Space));
    let keyboard = focus::keyboard_active(ui.ctx());
    let mut picked = None;
    for (slot, &at) in order.iter().enumerate() {
        let card = &cards[at];
        let rect = Rect::from_min_size(
            pos2(
                area.left() + (slot % columns) as f32 * (each + tile::GAP),
                area.top() + (slot / columns) as f32 * (tile::HEIGHT + tile::GAP),
            ),
            vec2(each, tile::HEIGHT),
        );
        let face = Tile {
            icon: icon_for(&card.category),
            name: &card.name,
            description: &card.description,
        };
        let focused = ui.memory(|memory| memory.has_focus(ids[slot]));
        let response = tile::show(ui, ids[slot], rect, &face, focused && keyboard, |ui| {
            control(ui, card)
        });
        if moved_to == Some(slot) {
            response.scroll_to_me(None);
        }
        let by_space = space && response.has_focus();
        if response.clicked() || by_space {
            let how = if by_space {
                Activation::Toggle
            } else {
                Activation::Open
            };
            picked = Some(Picked { card: at, how });
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::ThemeChoice;

    fn card(id: &str, name: &str, category: &str, description: &str) -> Card {
        Card {
            id: id.to_string(),
            name: name.to_string(),
            author: "community".to_string(),
            description: description.to_string(),
            category: category.to_string(),
        }
    }

    fn five() -> Vec<Card> {
        vec![
            card(
                "dim",
                "ScreenDimmer",
                "Monitors",
                "Cover monitors with overlays.",
            ),
            card(
                "keys",
                "ShortcutDetector",
                "Keyboard",
                "Find taken key combinations.",
            ),
            card(
                "layout",
                "LayoutKeeper",
                "Windows",
                "Remembers where each window is.",
            ),
            card(
                "lang",
                "LanguageIndicator",
                "Input",
                "Shows the input language.",
            ),
            card("namer", "WindowNamer", "Windows", "Give any window a name."),
        ]
    }

    fn names(cards: &[Card], order: &[usize]) -> Vec<String> {
        order.iter().map(|&at| cards[at].name.clone()).collect()
    }

    #[test]
    fn search_looks_in_name_description_and_category_and_needs_every_word() {
        let cards = five();
        let find = |query: &str| {
            let view = View {
                query: query.to_string(),
                ..View::default()
            };
            names(&cards, &visible(&cards, &view))
        };
        assert_eq!(find("").len(), 5);
        assert_eq!(find("dimmer"), ["ScreenDimmer"]);
        assert_eq!(find("KEYBOARD"), ["ShortcutDetector"]);
        assert_eq!(find("window"), ["LayoutKeeper", "WindowNamer"]);
        assert_eq!(find("window name"), ["WindowNamer"]);
        assert_eq!(find("input language"), ["LanguageIndicator"]);
        assert!(find("nothing like this").is_empty());
    }

    #[test]
    fn the_category_chip_and_the_search_work_together() {
        let cards = five();
        let mut view = View {
            category: Some("Windows".to_string()),
            ..View::default()
        };
        assert_eq!(
            names(&cards, &visible(&cards, &view)),
            ["LayoutKeeper", "WindowNamer"]
        );
        view.query = "namer".to_string();
        assert_eq!(names(&cards, &visible(&cards, &view)), ["WindowNamer"]);
        view.category = Some("Input".to_string());
        assert!(visible(&cards, &view).is_empty());
    }

    #[test]
    fn categories_are_counted_in_the_order_they_first_appear() {
        let cards = five();
        let show = |query: &str| -> Vec<(String, usize)> { counts(&cards, query) };
        let pairs = |list: &[(&str, usize)]| -> Vec<(String, usize)> {
            list.iter().map(|(n, c)| (n.to_string(), *c)).collect()
        };
        assert_eq!(
            show(""),
            pairs(&[
                ("Monitors", 1),
                ("Keyboard", 1),
                ("Windows", 2),
                ("Input", 1)
            ])
        );
        assert_eq!(
            show("window"),
            pairs(&[
                ("Monitors", 0),
                ("Keyboard", 0),
                ("Windows", 2),
                ("Input", 0)
            ])
        );
    }

    #[test]
    fn a_missing_category_is_other() {
        assert_eq!(category_name("  "), OTHER);
        assert_eq!(category_name(" Input "), "Input");
        let entry = IndexEntry {
            id: "x".to_string(),
            name: "X".to_string(),
            version: "1.0.0".to_string(),
            author: "a".to_string(),
            description: String::new(),
            readme: String::new(),
            category: String::new(),
            thumbnail: None,
        };
        assert_eq!(Card::from_entry(&entry, None).category, OTHER);
    }

    #[test]
    fn sorting_by_name_or_category_ignores_case_and_keeps_ties_in_name_order() {
        let cards = five();
        let mut view = View::sorted_by(Sort::Name);
        assert_eq!(
            names(&cards, &visible(&cards, &view)),
            [
                "LanguageIndicator",
                "LayoutKeeper",
                "ScreenDimmer",
                "ShortcutDetector",
                "WindowNamer"
            ]
        );
        view.sort = Sort::Category;
        assert_eq!(
            names(&cards, &visible(&cards, &view)),
            [
                "LanguageIndicator",
                "ShortcutDetector",
                "ScreenDimmer",
                "LayoutKeeper",
                "WindowNamer"
            ]
        );
    }

    #[test]
    fn a_store_tile_shows_the_badge_and_button_for_its_state() {
        let on = StoreState::of(Some(true), "0.20.2", "0.20.2");
        assert_eq!(on, StoreState::Enabled);
        assert_eq!(
            on.badge(),
            Some(BadgeSpec {
                label: "Enabled".to_string(),
                tone: Tone::Good
            })
        );
        assert_eq!(on.button(), None);

        let off = StoreState::of(Some(false), "0.20.2", "0.20.2");
        assert_eq!(off.badge().unwrap().label, "Installed, disabled");
        assert_eq!(off.badge().unwrap().tone, Tone::Quiet);
        let enable = off.button().unwrap();
        assert_eq!(
            (enable.label, enable.kind, enable.action),
            ("Enable", Kind::Primary, StoreAction::Enable)
        );

        let newer = StoreState::of(None, "0.21.0", "0.20.2");
        assert_eq!(newer, StoreState::NeedsUpdate("0.21.0".to_string()));
        assert_eq!(newer.badge().unwrap().label, "Needs WinCraft 0.21.0");
        assert_eq!(newer.badge().unwrap().tone, Tone::Warn);
        let update = newer.button().unwrap();
        assert_eq!(
            (update.label, update.kind, update.action),
            ("Get update", Kind::Secondary, StoreAction::OpenReleases)
        );

        let missing = StoreState::of(None, "0.20.2", "0.20.2");
        assert_eq!(missing, StoreState::NotInstalled);
        assert_eq!(missing.badge(), None);
        let install = missing.button().unwrap();
        assert_eq!((install.label, install.kind), ("Install", Kind::Primary));
    }

    #[test]
    fn a_store_tile_ends_in_installed_install_or_get_update() {
        let installed = |on| StoreState::of(Some(on), "0.20.2", "0.20.2").tile_control();
        assert_eq!(installed(true), TileControl::Installed);
        assert_eq!(installed(false), TileControl::Installed);

        let label = |control| match control {
            TileControl::Button(spec) => (spec.label, spec.kind, spec.action),
            TileControl::Installed => panic!("expected a button"),
        };
        assert_eq!(
            label(StoreState::of(None, "0.20.2", "0.20.2").tile_control()),
            ("Install", Kind::Primary, StoreAction::OpenReleases)
        );
        assert_eq!(
            label(StoreState::of(None, "0.21.0", "0.20.2").tile_control()),
            ("Get update", Kind::Secondary, StoreAction::OpenReleases)
        );
    }

    #[test]
    fn arrows_walk_the_tiles_row_by_row_and_stop_at_the_edges() {
        // Seven tiles, three to a row:  0 1 2 / 3 4 5 / 6
        assert_eq!(step(0, Move::Right, 3, 7), 1);
        assert_eq!(step(2, Move::Right, 3, 7), 3, "Right wraps to the next row");
        assert_eq!(step(6, Move::Right, 3, 7), 6);
        assert_eq!(step(0, Move::Left, 3, 7), 0);
        assert_eq!(step(3, Move::Left, 3, 7), 2);
        assert_eq!(step(1, Move::Down, 3, 7), 4);
        assert_eq!(step(4, Move::Down, 3, 7), 6, "Down onto a short last row");
        assert_eq!(step(6, Move::Down, 3, 7), 6);
        assert_eq!(step(4, Move::Up, 3, 7), 1);
        assert_eq!(step(1, Move::Up, 3, 7), 1);
        assert_eq!(step(4, Move::First, 3, 7), 0);
        assert_eq!(step(4, Move::Last, 3, 7), 6);
        assert_eq!(step(0, Move::Down, 1, 4), 1, "one column is a list");
        assert_eq!(step(0, Move::Right, 3, 0), 0);
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    /// Draws the five tiles in an 894 px wide window (three to a row) and
    /// reports what was picked, whether Arrow Down was still in the input
    /// after the grid, and the id the grid was drawn under.
    fn frame(
        ctx: &egui::Context,
        cards: &[Card],
        events: Vec<egui::Event>,
    ) -> (Option<Picked>, bool, Id) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(894.0, 2000.0))),
            events,
            ..Default::default()
        };
        let order: Vec<usize> = (0..cards.len()).collect();
        let mut result = (None, false, Id::NULL);
        let mut output = ctx.run_ui(input, |ui| {
            let picked = tiles(ui, cards, &order, |_, _| {});
            let arrow_left = ui.input(|input| input.key_pressed(Key::ArrowDown));
            result = (picked, arrow_left, ui.id());
        });
        output.textures_delta.clear();
        result
    }

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(
            &ctx,
            crate::core::config::ThemeName::Classic,
            ThemeChoice::Dark,
        );
        ctx
    }

    #[test]
    fn arrows_move_focus_between_tiles_and_space_and_enter_pick_the_focused_one() {
        let ctx = context();
        let cards = five();
        let (_, _, grid) = frame(&ctx, &cards, Vec::new());
        let id = |at: usize| tile_id(grid, &cards[at]);
        let focused = || ctx.memory(|memory| memory.focused());

        ctx.memory_mut(|memory| memory.request_focus(id(0)));
        for _ in 0..3 {
            frame(&ctx, &cards, Vec::new());
        }
        assert_eq!(focused(), Some(id(0)));

        frame(&ctx, &cards, vec![key(Key::ArrowRight)]);
        assert_eq!(focused(), Some(id(1)));
        frame(&ctx, &cards, Vec::new());
        let (_, still_there, _) = frame(&ctx, &cards, vec![key(Key::ArrowDown)]);
        assert_eq!(focused(), Some(id(4)), "one row down, same column");
        assert!(!still_there, "the grid keeps the arrow from the side menu");
        frame(&ctx, &cards, Vec::new());
        frame(&ctx, &cards, vec![key(Key::ArrowLeft)]);
        assert_eq!(focused(), Some(id(3)));

        let (picked, _, _) = frame(&ctx, &cards, vec![key(Key::Space)]);
        assert_eq!(
            picked,
            Some(Picked {
                card: 3,
                how: Activation::Toggle
            })
        );
        let (picked, _, _) = frame(&ctx, &cards, vec![key(Key::Enter)]);
        assert_eq!(
            picked,
            Some(Picked {
                card: 3,
                how: Activation::Open
            })
        );
    }

    #[test]
    fn arrows_are_left_for_the_side_menu_when_no_tile_has_focus() {
        let ctx = context();
        let cards = five();
        frame(&ctx, &cards, Vec::new());
        let (_, still_there, _) = frame(&ctx, &cards, vec![key(Key::ArrowDown)]);
        assert!(still_there);
    }
}
