//! Who gets an Esc in WindowDeck. Esc goes to the first of: a title edit or
//! a card being dragged back (their own code takes it); the Claude board;
//! an open group or its dialog (the group code takes it, and WindowDeck
//! stays open); the lit Claude status. Only when none of them is there does
//! Esc close WindowDeck.

/// What is open in WindowDeck when Esc comes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Open {
    pub rename: bool,
    pub putting_back: bool,
    pub board: bool,
    /// An open group, a group's name box or its close question.
    pub group: bool,
    pub status: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeTo {
    /// A title edit or a drag takes it in its own code.
    Edit,
    Board,
    /// The group code closes the dialog, or else the group.
    Group,
    Status,
    Close,
}

pub fn escape_to(open: Open) -> EscapeTo {
    if open.rename || open.putting_back {
        EscapeTo::Edit
    } else if open.board {
        EscapeTo::Board
    } else if open.group {
        EscapeTo::Group
    } else if open.status {
        EscapeTo::Status
    } else {
        EscapeTo::Close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The user saw Esc close an open group and WindowDeck with it (0.20.91).
    #[test]
    fn esc_closes_an_open_group_and_then_windowdeck() {
        let mut open = Open {
            group: true,
            ..Open::default()
        };
        assert_eq!(escape_to(open), EscapeTo::Group, "first Esc: the group");
        open.group = false;
        assert_eq!(escape_to(open), EscapeTo::Close, "second Esc: WindowDeck");
    }

    #[test]
    fn esc_goes_to_the_first_thing_open() {
        let all = Open {
            rename: true,
            putting_back: false,
            board: true,
            group: true,
            status: true,
        };
        assert_eq!(escape_to(all), EscapeTo::Edit);
        let open = Open {
            rename: false,
            ..all
        };
        assert_eq!(escape_to(open), EscapeTo::Board);
        let open = Open {
            board: false,
            ..open
        };
        assert_eq!(escape_to(open), EscapeTo::Group);
        let open = Open {
            group: false,
            ..open
        };
        assert_eq!(escape_to(open), EscapeTo::Status);
        let drag = Open {
            putting_back: true,
            ..Open::default()
        };
        assert_eq!(escape_to(drag), EscapeTo::Edit);
    }
}
