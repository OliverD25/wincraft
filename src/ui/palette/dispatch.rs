use super::*;

impl Palette {
    /// `appear` runs from 0 to 1 while the palette fades in, and back to 0 as
    /// it fades out; `interactive` is false during the fade out.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
        appear: f32,
        interactive: bool,
    ) -> Outcome {
        // While the dialog is open the list stays still: it neither acts on
        // keys nor hides when the window loses focus, which the dialog
        // answers itself by cancelling.
        let editing = self.hotkey_dialog.is_some();
        let outcome = self.show_view(ui, snapshot, to_host, appear, interactive && !editing);
        self.show_hotkey_dialog(ui.ctx(), snapshot, to_host);
        outcome
    }

    /// Ctrl+K's dialog over the panel; a save goes to the host at once.
    fn show_hotkey_dialog(
        &mut self,
        ctx: &egui::Context,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
    ) {
        let Some(editor) = self.hotkey_dialog.as_mut() else {
            return;
        };
        let radius = Tokens::get(ctx).radius_window;
        let Some(outcome) =
            hotkey_dialog::show(ctx, editor, &snapshot.shortcuts, self.panel, radius)
        else {
            return;
        };
        if let hotkey_dialog::Outcome::Save(changes) = outcome {
            to_host.send(HostRequest::SetShortcuts(changes));
            self.searched = None;
        }
        self.hotkey_dialog = None;
    }

    /// The live picture of the selected window, drawn by DWM on top of the
    /// palette's own window in the inspector's preview box.
    pub(super) fn place_thumbnail(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        tokens: &Tokens,
        preview: Option<(isize, Rect)>,
        appear: f32,
    ) {
        let Some((hwnd, rect)) = preview.filter(|_| self.window != 0) else {
            self.thumbnail = None;
            return;
        };
        if self
            .thumbnail
            .as_ref()
            .is_none_or(|(known, _)| *known != hwnd)
        {
            self.thumbnail = Thumbnail::register(self.window as HWND, hwnd as HWND)
                .map(|thumbnail| (hwnd, thumbnail));
        }
        let Some((_, thumbnail)) = self.thumbnail.as_mut() else {
            inspector::no_picture(painter, rect, tokens);
            return;
        };
        let Some(size) = thumbnail.source_size() else {
            thumbnail.place(None);
            inspector::no_picture(painter, rect, tokens);
            return;
        };
        let source = dwm_thumbs::picture_source(dwm_thumbs::visible_frame(hwnd as HWND), size);
        let fitted = dwm_thumbs::fit(source.size(), rect.size());
        let dest = Rect::from_center_size(rect.center(), fitted);
        let placement = dwm_thumbs::crop(dest, rect, source).map(|(shown, cropped)| Placement {
            dest: dwm_thumbs::physical(shown, ctx.pixels_per_point()),
            source: dwm_thumbs::physical(cropped, 1.0),
            opacity: (appear.clamp(0.0, 1.0) * 255.0).round() as u8,
        });
        thumbnail.place(placement);
    }

    /// Runs what a key, a click or the inspector chose.
    pub(super) fn run(
        &mut self,
        run: Run,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
    ) -> Outcome {
        match run {
            Run::Action(action) => self.perform(action, snapshot, to_host),
            Run::Complete(text) => {
                self.set_query(text);
                Outcome::Stay
            }
            Run::Forget => {
                if let Some(key) = self.selected_item().and_then(|item| item.usage_key.clone()) {
                    self.router
                        .forget_pick(&key, self.prefix.as_deref(), &self.query);
                    self.searched = None;
                    self.action_cursor = None;
                }
                Outcome::Stay
            }
            Run::Mark => {
                if let Some(item) = self.selected_item().cloned() {
                    let place = Self::window_place(&item);
                    self.marks.toggle(&item, || place);
                }
                Outcome::Stay
            }
            Run::HideUpdate(tag) => {
                self.hide_pinned(tag, to_host);
                Outcome::Stay
            }
        }
    }

    /// What choosing a row, or pressing one of its keys, does.
    pub(super) fn perform(
        &mut self,
        action: Action,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
    ) -> Outcome {
        match action {
            Action::OpenPlugin(plugin) => Outcome::OpenPlugin(plugin),
            Action::ChangeShortcut { id, label } => match Target::find(&snapshot.shortcuts, &id) {
                Some(target) => {
                    // The old dialog's drop removes the hook, so it goes
                    // before the new one starts it.
                    self.hotkey_dialog = None;
                    self.hotkey_dialog = Some(Editor::open(target));
                    Outcome::Stay
                }
                None => Outcome::OpenShortcut { id, label },
            },
            Action::SetPrefix(prefix) => {
                self.set_prefix(Some(prefix).filter(|prefix| !prefix.is_empty()));
                Outcome::Stay
            }
            Action::Provider { provider, command } => {
                match self.router.act(&provider, &command, &context(snapshot)) {
                    Reply::Stay => {
                        self.searched = None;
                        Outcome::Stay
                    }
                    Reply::ClearQuery => {
                        self.set_query(String::new());
                        Outcome::Stay
                    }
                    Reply::Switch { provider, text } => {
                        if let Some(prefix) = self.router.prefix_of(provider) {
                            self.prefix = Some(prefix);
                            self.set_query(text);
                        }
                        Outcome::Stay
                    }
                    Reply::Ask(ask) => {
                        self.ask_for(ask);
                        Outcome::Stay
                    }
                    Reply::Hide => Outcome::Hide,
                }
            }
            Action::Command(id) => {
                let entry = snapshot.commands.iter().find(|entry| entry.id == id);
                if entry.is_some_and(|entry| entry.list) {
                    self.open_sub_list(id, to_host);
                    return Outcome::Stay;
                }
                if !self.confirmed(snapshot, id, "") {
                    return Outcome::Stay;
                }
                to_host.send(HostRequest::RunCommand(id));
                // A toggle flips in place: the palette stays open and its
                // row shows the new state when the host has read it.
                if entry.is_some_and(|entry| entry.toggle.is_some()) {
                    Outcome::Stay
                } else {
                    Outcome::Hide
                }
            }
            Action::ListPick { command, row } => {
                if !self.confirmed(snapshot, command, &row) {
                    return Outcome::Stay;
                }
                self.sub_list = None;
                to_host.send(HostRequest::ListPick { id: command, row });
                Outcome::Hide
            }
            other => {
                to_host.send(HostRequest::RunAction(other));
                Outcome::Hide
            }
        }
    }
}
