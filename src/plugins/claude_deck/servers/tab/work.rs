//! The rows of "Work from the PC" on a server's card: the chat's name (or
//! "verdict-ai chats"), the job's name, and at the right its run time,
//! CPU and RAM; an ended job greyed, with how it ended. Resting the mouse
//! shows the command and the folder. A click goes to the chat; the menu
//! has "Show the chat" and the copy items.

use egui::{Painter, Rect, Response};

use crate::plugins::claude_deck::rows::Look;

use super::super::draw::{copy, OFFLINE_OPACITY};
use super::super::jobs::text;
use super::draw::{back, row_words, INDENT};
use super::view::{self, Line};

/// Draws a job's row and its menu. Returns the chat to show when the
/// row was clicked or "Show the chat" was picked.
pub fn row(
    painter: &Painter,
    rect: Rect,
    line: &Line,
    look: &Look,
    looks: (bool, bool),
    response: &Response,
    window: isize,
) -> Option<String> {
    let tokens = look.tokens;
    let (chat, owner, name, right, command, folder, pid) = match line {
        Line::Work { job, .. } => (
            job.chats.first().map(|(id, _)| id.clone()),
            job.owner(),
            job.name.clone(),
            text::job_right(job, look.now),
            job.command.clone(),
            job.folder.clone(),
            job.pid,
        ),
        Line::Ended { end, .. } => (
            end.chat.clone(),
            end.owner.clone(),
            end.name.clone(),
            text::ended_right(end),
            end.command.clone(),
            end.folder.clone(),
            end.pid,
        ),
        _ => return None,
    };
    back(painter, rect, tokens, looks);
    let mut words = painter.clone();
    let colour = if matches!(line, Line::Ended { .. }) {
        words.multiply_opacity(OFFLINE_OPACITY);
        tokens.text_secondary
    } else {
        tokens.text_primary
    };
    row_words(
        &words,
        rect,
        rect.left() + INDENT,
        (&owner, colour),
        &name,
        &right,
        tokens,
    );
    let mut show = response.clicked().then(|| chat.clone()).flatten();
    egui::Popup::context_menu(response).show(|ui| {
        if let Some(id) = &chat {
            if ui
                .add(egui::Button::new(text::SHOW_CHAT).shortcut_text("Enter"))
                .clicked()
            {
                show = Some(id.clone());
                ui.close();
            }
        }
        if ui
            .add(egui::Button::new(view::COPY_COMMAND).shortcut_text("Ctrl+C"))
            .clicked()
        {
            copy(window, &command);
            ui.close();
        }
        if let Some(pid) = pid {
            if ui.button(view::COPY_PID).clicked() {
                copy(window, &pid.to_string());
                ui.close();
            }
        }
    });
    response
        .clone()
        .on_hover_text(text::row_hover(&command, folder.as_deref(), pid));
    show
}
