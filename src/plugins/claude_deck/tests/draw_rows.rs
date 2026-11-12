//! The status marks and the rows, painted on a headless screen.

use egui::{pos2, vec2, Rect};

use super::screen::Screen;
use super::*;
use crate::core::config::ThemeChoice;
use crate::plugins::claude_deck::header::{self, Click};
use crate::plugins::claude_deck::marks;
use crate::plugins::claude_deck::rows::{self, GroupState, Look};
use crate::plugins::claude_deck::store::Colour;
use crate::plugins::claude_deck::{MarkStyle, Settings};

const ALL_MARKS: [Mark; 11] = [
    Mark::NeedsApproval,
    Mark::AsksYou,
    Mark::DoneNotSeen,
    Mark::Working,
    Mark::Background,
    Mark::Idle(Age::Fresh),
    Mark::Idle(Age::Cooling),
    Mark::Idle(Age::Cold),
    Mark::Idle(Age::Frozen),
    Mark::Stopped,
    Mark::Failed,
];

fn line_of(chat: Chat) -> ChatLine {
    ChatLine {
        chat,
        stand_in: false,
        depth: 0,
        rails: Vec::new(),
        last: false,
        fold: Fold::None,
        pinned: false,
        hidden: false,
    }
}

fn row_rect() -> Rect {
    Rect::from_min_size(pos2(0.0, 40.0), vec2(360.0, 34.0))
}

#[test]
fn every_mark_paints_in_every_style_and_the_words_come_with_the_words_style() {
    for mode in [ThemeChoice::Light, ThemeChoice::Dark] {
        let mut screen = Screen::new(mode);
        let tokens = screen.tokens();
        for mark in ALL_MARKS.into_iter().chain([Mark::Unknown]) {
            for style in [MarkStyle::Off, MarkStyle::Shapes, MarkStyle::ShapesAndWords] {
                let mut used = 0.0;
                let painted = screen.frame(Vec::new(), |ui| {
                    used = marks::paint(ui.painter(), pos2(10.0, 10.0), mark, style, &tokens, 0.4);
                });
                match style {
                    MarkStyle::Off => {
                        assert_eq!(used, 0.0, "{mark:?}");
                        assert_eq!(painted.shapes, 0);
                    }
                    MarkStyle::Shapes => {
                        assert_eq!(used, marks::SIZE);
                        assert!(painted.shapes > 0, "{mark:?} paints a shape");
                        assert!(painted.texts.is_empty());
                    }
                    MarkStyle::ShapesAndWords => {
                        assert!(used >= marks::WORD_WIDTH, "names line up");
                        assert!(painted.has(mark.word()), "{mark:?}: {:?}", painted.texts);
                    }
                }
            }
        }
    }
}

#[test]
fn a_mark_has_its_own_colour_and_the_widths_are_the_styles() {
    let screen = Screen::new(ThemeChoice::Dark);
    let tokens = screen.tokens();
    assert_eq!(marks::colour(Mark::NeedsApproval, &tokens), tokens.warning);
    assert_eq!(marks::colour(Mark::AsksYou, &tokens), tokens.warning);
    assert_eq!(marks::colour(Mark::DoneNotSeen, &tokens), tokens.success);
    assert_eq!(marks::colour(Mark::Working, &tokens), tokens.info);
    assert_eq!(marks::colour(Mark::Failed, &tokens), tokens.danger);
    assert_eq!(marks::colour(Mark::Idle(Age::Frozen), &tokens), tokens.info);
    assert_eq!(
        marks::colour(Mark::Idle(Age::Cooling), &tokens),
        tokens.text_disabled
    );
    assert_eq!(marks::width(MarkStyle::Off), 0.0);
    assert_eq!(marks::width(MarkStyle::Shapes), marks::SIZE);
    assert_eq!(marks::width(MarkStyle::ShapesAndWords), marks::WORD_WIDTH);
}

#[test]
fn the_working_ring_turns_with_the_clock() {
    let mut screen = Screen::new(ThemeChoice::Dark);
    let tokens = screen.tokens();
    let mut count = Vec::new();
    for turn in [0.0, 0.25, 0.9] {
        let painted = screen.frame(Vec::new(), |ui| {
            marks::shape(
                ui.painter(),
                Rect::from_min_size(pos2(0.0, 0.0), vec2(14.0, 14.0)),
                Mark::Working,
                &tokens,
                turn,
            );
        });
        count.push(painted.shapes);
    }
    assert!(count.iter().all(|n| *n == count[0] && *n >= 2), "{count:?}");
    let painted = screen.frame(Vec::new(), |ui| {
        marks::stand_in(
            ui.painter(),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(14.0, 14.0)),
            &tokens,
        );
    });
    assert!(painted.shapes >= 4, "a dashed square has several dashes");
}

#[test]
fn a_chat_row_shows_its_name_branch_time_jobs_and_text_line() {
    let mut screen = Screen::new(ThemeChoice::Dark);
    let tokens = screen.tokens();
    let mut one = chat("a", Some(r"E:\code\WinCraft"), 1);
    one.jobs = 2;
    one.model = Some("claude-opus-5-5".into());
    one.text = Some("Run the tests?".into());
    let line = line_of(one);
    let full = Settings {
        model: true,
        ..Settings::default()
    };
    let mut paint = |settings: &Settings, selected: bool, hovered: bool, key: Option<u8>| {
        let look = Look {
            tokens: &tokens,
            settings,
            now: NOW,
            turn: 0.0,
        };
        screen.frame(Vec::new(), |ui| {
            rows::chat(
                ui.painter(),
                row_rect(),
                &line,
                &look,
                hovered,
                selected,
                key,
            );
        })
    };
    let plain = paint(&full, false, false, None);
    assert!(plain.has("Chat a"));
    assert!(plain.has("main"), "the branch");
    assert!(plain.has("Run the tests?"), "the text line");
    let hovered = paint(&full, false, true, None);
    let selected = paint(&full, true, false, Some(3));
    assert!(hovered.shapes > plain.shapes, "a hover fill");
    assert!(selected.shapes > hovered.shapes, "a fill and a border");
    assert!(selected.has("3"), "the digit's box");

    let bare = Settings {
        text: false,
        time: false,
        project: false,
        jobs: false,
        mark: MarkStyle::Off,
        ..Settings::default()
    };
    let painted = paint(&bare, false, false, None);
    assert!(painted.has("Chat a"));
    assert!(!painted.has("Run the tests?"));
    assert!(!painted.has("main"));
    assert!(painted.shapes < plain.shapes);
}

#[test]
fn forks_stand_ins_and_folded_parents_are_drawn_differently() {
    let mut screen = Screen::new(ThemeChoice::Light);
    let tokens = screen.tokens();
    let look_settings = Settings::default();
    let mut shapes = |line: &ChatLine, settings: &Settings| {
        let look = Look {
            tokens: &tokens,
            settings,
            now: NOW,
            turn: 0.0,
        };
        screen.frame(Vec::new(), |ui| {
            rows::chat(ui.painter(), row_rect(), line, &look, false, false, None);
        })
    };
    let top = line_of(chat("p", Some(r"E:\code\WinCraft"), 1));
    let mut child = line_of(fork("f", "p", 2));
    child.depth = 1;
    child.rails = vec![true];
    let mut last = child.clone();
    last.last = true;
    let mut open = top.clone();
    open.fold = Fold::Open;
    let mut folded = top.clone();
    folded.fold = Fold::Folded(3);
    let mut stand = top.clone();
    stand.stand_in = true;
    stand.chat.title = "Old one \u{b7} archived".into();
    let mut stand_folded = stand.clone();
    stand_folded.fold = Fold::Folded(2);
    let mut hidden = top.clone();
    hidden.hidden = true;

    let base = shapes(&top, &look_settings);
    assert!(
        shapes(&child, &look_settings).shapes > base.shapes,
        "tree line"
    );
    assert!(shapes(&last, &look_settings).shapes > 0);
    assert!(
        shapes(&open, &look_settings).shapes > base.shapes,
        "chevron"
    );
    let f = shapes(&folded, &look_settings);
    assert!(f.has("3"), "the number of hidden forks: {:?}", f.texts);
    let s = shapes(&stand, &look_settings);
    assert!(s.has("archived"));
    assert!(shapes(&stand_folded, &look_settings).has("Old one"));
    assert!(shapes(&hidden, &look_settings).has("Chat p"));
    let off = Settings {
        mark: MarkStyle::Off,
        ..Settings::default()
    };
    assert!(shapes(&stand, &off).has("Old one"));
    let words = Settings {
        mark: MarkStyle::ShapesAndWords,
        ..Settings::default()
    };
    assert!(shapes(&top, &words).has("Idle"));
}

#[test]
fn a_long_name_is_cut_and_a_narrow_row_does_not_panic() {
    let mut screen = Screen::new(ThemeChoice::Dark);
    let tokens = screen.tokens();
    let settings = Settings {
        model: true,
        ..Settings::default()
    };
    let mut long = chat("l", Some(r"E:\code\WinCraft"), 1);
    long.title = "A very long name that cannot fit in a narrow window at all".into();
    long.jobs = 12;
    long.text = Some("x".repeat(300));
    let mut line = line_of(long);
    line.fold = Fold::Folded(9);
    for width in [40.0, 90.0, 160.0, 360.0] {
        let look = Look {
            tokens: &tokens,
            settings: &settings,
            now: NOW,
            turn: 0.1,
        };
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 34.0));
        let painted = screen.frame(Vec::new(), |ui| {
            rows::chat(ui.painter(), rect, &line, &look, false, false, Some(1));
        });
        assert!(painted.shapes > 0, "width {width}");
    }
}

fn group_line(name: &str, folded: bool, colour: Option<Colour>, roll: Mark) -> GroupLine {
    GroupLine {
        key: "g1".into(),
        name: name.into(),
        colour,
        chats: 4,
        folded,
        roll,
        roll_since: Some(NOW - MIN),
    }
}

#[test]
fn group_headers_open_folded_coloured_and_being_renamed() {
    let mut screen = Screen::new(ThemeChoice::Dark);
    let tokens = screen.tokens();
    let settings = Settings::default();
    let off = Settings {
        mark: MarkStyle::Off,
        ..Settings::default()
    };
    let rect = Rect::from_min_size(pos2(0.0, 30.0), vec2(360.0, 24.0));
    let mut draw = |line: &GroupLine, set: &Settings, state: GroupState| {
        let look = Look {
            tokens: &tokens,
            settings: set,
            now: NOW,
            turn: 0.0,
        };
        let mut room = Rect::NOTHING;
        let painted = screen.frame(Vec::new(), |ui| {
            room = rows::group(ui.painter(), rect, line, &look, state);
        });
        (painted, room)
    };
    let (open, _) = draw(
        &group_line("Alpha", false, None, Mark::Working),
        &settings,
        GroupState::default(),
    );
    assert!(open.has("Alpha") && open.has("4"));
    let (folded, _) = draw(
        &group_line("Alpha", true, None, Mark::Working),
        &settings,
        GroupState::default(),
    );
    assert!(folded.has("4 chats"), "{:?}", folded.texts);
    assert!(folded.shapes > open.shapes, "the urgent mark inside");
    let (no_mark, _) = draw(
        &group_line("Alpha", true, None, Mark::Working),
        &off,
        GroupState::default(),
    );
    assert!(no_mark.shapes < folded.shapes);
    for colour in Colour::ALL {
        let (dotted, _) = draw(
            &group_line("Beta", false, Some(colour), Mark::Idle(Age::Fresh)),
            &settings,
            GroupState {
                hovered: true,
                ..GroupState::default()
            },
        );
        assert!(dotted.shapes > open.shapes, "{colour:?}: a dot and a hover");
    }
    let (picked, room) = draw(
        &group_line("Alpha", false, None, Mark::Working),
        &settings,
        GroupState {
            selected: true,
            ..GroupState::default()
        },
    );
    assert!(picked.shapes > open.shapes);
    let (renaming, field) = draw(
        &group_line("Alpha", false, None, Mark::Working),
        &settings,
        GroupState {
            renaming: true,
            ..GroupState::default()
        },
    );
    assert!(!renaming.has("Alpha"), "the field takes the name's place");
    assert!(field.width() > 0.0 && room.width() > 0.0);
}

#[test]
fn the_cold_the_hidden_and_the_drop_lines() {
    let mut screen = Screen::new(ThemeChoice::Dark);
    let tokens = screen.tokens();
    let settings = Settings::default();
    let look = Look {
        tokens: &tokens,
        settings: &settings,
        now: NOW,
        turn: 0.0,
    };
    let rect = Rect::from_min_size(pos2(0.0, 30.0), vec2(360.0, 28.0));
    let cold_line = ColdLine {
        chats: 5,
        days: (2, 30),
        open: false,
    };
    let plain = screen.frame(Vec::new(), |ui| {
        rows::cold(ui.painter(), rect, &cold_line, &look, false)
    });
    let hovered = screen.frame(Vec::new(), |ui| {
        rows::cold(ui.painter(), rect, &cold_line, &look, true)
    });
    assert!(plain.has(&cold_label(5)), "{:?}", plain.texts);
    assert_eq!(hovered.shapes, plain.shapes + 1);
    for open in [false, true] {
        let line = HiddenLine { chats: 2, open };
        let a = screen.frame(Vec::new(), |ui| {
            rows::hidden(ui.painter(), rect, &line, &look, false)
        });
        let b = screen.frame(Vec::new(), |ui| {
            rows::hidden(ui.painter(), rect, &line, &look, true)
        });
        assert!(a.has(&hidden_label(2)), "{:?}", a.texts);
        assert_eq!(b.shapes, a.shapes + 1);
    }
    let dropped = screen.frame(Vec::new(), |ui| {
        rows::drop_line(ui.painter(), rect, 40.0, &tokens)
    });
    assert_eq!(dropped.shapes, 1);
    assert_eq!(rows::colour_of(Colour::Red, &tokens), tokens.danger);
    assert_eq!(
        rows::colour_of(Colour::Grey, &tokens),
        tokens.text_secondary
    );
}

#[test]
fn row_heights_and_the_drawn_body() {
    let g = Line::Group(group_line("A", false, None, Mark::Idle(Age::Fresh)));
    let c = Line::Chat(Box::new(line_of(chat("a", None, 1))));
    let k = Line::Cold(ColdLine {
        chats: 1,
        days: (1, 1),
        open: false,
    });
    let h = Line::Hidden(HiddenLine {
        chats: 1,
        open: false,
    });
    assert_eq!(rows::height(&c, false, false), rows::ROW);
    assert_eq!(rows::height(&c, false, true), rows::ROW_TWO);
    assert_eq!(rows::height(&g, true, true), rows::GROUP);
    assert_eq!(rows::height(&g, false, true), rows::GROUP + rows::GROUP_GAP);
    assert_eq!(
        rows::height(&k, false, false),
        rows::GROUP + rows::GROUP_GAP
    );
    assert_eq!(rows::height(&h, true, false), rows::GROUP + rows::GROUP_GAP);
    let whole = Rect::from_min_size(pos2(0.0, 100.0), vec2(300.0, 28.0));
    assert!(rows::body(&g, whole).height() <= whole.height());
    assert_eq!(rows::body(&c, whole), whole, "a chat row has no gap above");
    let chevron = rows::chevron_rect(whole, 1);
    assert!(chevron.left() > rows::chevron_rect(whole, 0).left());
}

#[test]
fn the_header_counts_the_switch_and_the_five_buttons() {
    let mut screen = Screen::new(ThemeChoice::Dark);
    let tokens = screen.tokens();
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, header::HEIGHT));
    let counts = vec![
        (Mark::NeedsApproval, 2),
        (Mark::AsksYou, 1),
        (Mark::Working, 3),
        (Mark::DoneNotSeen, 4),
    ];
    let mut got: Vec<Option<Click>> = Vec::new();
    let state = Settings::default();
    let mut gear_clicks = 0;
    let mut draw = |ui: &mut egui::Ui| {
        let mut quick = crate::plugins::claude_deck::quick::Quick::default();
        let clicked = header::show(ui, rect, &counts[..3], &state, &tokens, 0.0, &mut quick);
        if quick.toggle {
            gear_clicks += 1;
        }
        if clicked.is_some() {
            got.push(clicked);
        }
    };
    let painted = screen.settle(&mut draw);
    assert!(painted.has(header::ONLY_OPEN));
    assert!(painted.has("2") && painted.has("3"), "{:?}", painted.texts);
    let middle = header::HEIGHT / 2.0;
    let close = pos2(360.0 - 4.0 - 11.0, middle);
    let mini = pos2(close.x - 3.0 - 22.0, middle);
    let pin = pos2(mini.x - 3.0 - 22.0, middle);
    let gear = pos2(pin.x - 3.0 - 22.0, middle);
    let label = pos2(gear.x - 11.0 - 8.0 - 8.0, middle);
    screen.click(close, &mut draw);
    screen.click(mini, &mut draw);
    screen.click(pin, &mut draw);
    screen.click(gear, &mut draw);
    screen.click(label, &mut draw);
    assert_eq!(
        got,
        [
            Some(Click::Close),
            Some(Click::Mini),
            Some(Click::Pin),
            Some(Click::OnlyOpen)
        ]
    );
    assert_eq!(gear_clicks, 1, "the gear asks for the quick settings");

    // At the window's default width the four counts come first: the
    // switch's label goes, and a click on the switch still works.
    let narrow = Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, header::HEIGHT));
    let mut got = Vec::new();
    let mut draw = |ui: &mut egui::Ui| {
        let clicked = header::show(
            ui,
            narrow,
            &counts,
            &state,
            &tokens,
            0.0,
            &mut Default::default(),
        );
        got.extend(clicked);
    };
    let mut screen = Screen::new(ThemeChoice::Dark);
    let painted = screen.settle(&mut draw);
    assert!(
        painted.has("4") && !painted.has(header::ONLY_OPEN),
        "{:?}",
        painted.texts
    );
    let switch = pos2(360.0 - 4.0 - 4.0 * 22.0 - 3.0 * 3.0 - 8.0 - 12.0, middle);
    screen.click(switch, &mut draw);
    assert_eq!(got, [Click::OnlyOpen]);
    let off_state = Settings {
        only_open: true,
        stay_on_top: false,
        mark: MarkStyle::Off,
        ..Settings::default()
    };
    let on = Screen::new(ThemeChoice::Light).frame(Vec::new(), |ui| {
        header::show(
            ui,
            rect,
            &counts,
            &off_state,
            &tokens,
            0.5,
            &mut Default::default(),
        );
    });
    assert!(on.shapes > 0);
    let words = Settings {
        mark: MarkStyle::ShapesAndWords,
        ..Settings::default()
    };
    let narrow = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, header::HEIGHT));
    let few = Screen::new(ThemeChoice::Light).frame(Vec::new(), |ui| {
        header::show(
            ui,
            narrow,
            &counts,
            &words,
            &tokens,
            0.5,
            &mut Default::default(),
        );
    });
    assert!(
        few.shapes > 0 && !few.has("4"),
        "the counts that do not fit are left out"
    );
}
