//! The on-screen panel. For now it only logs what it would show.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Position {
    #[default]
    Centre,
    UpperThird,
    LowerThird,
}

pub struct Content {
    /// The code switched from, such as "UK"; None shows the target alone.
    pub from: Option<String>,
    pub to: String,
    /// The target's own name, with the layout when it is shown.
    pub detail: String,
}

pub struct Panel;

impl Panel {
    pub fn create() -> Result<Self, String> {
        Ok(Self)
    }

    pub fn show(&mut self, content: &Content, position: Position) {
        log::debug!(
            "panel at {position:?}: {} {} ({})",
            content.from.as_deref().unwrap_or("-"),
            content.to,
            content.detail
        );
    }

    pub fn set_alpha(&mut self, alpha: u8) {
        let _ = alpha;
    }

    pub fn hide(&mut self) {}

    pub fn destroy(&mut self) {}
}
