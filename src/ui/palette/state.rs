use super::*;

impl Palette {
    /// A search provider's question, such as the phrase for a bookmark.
    pub(super) fn ask_for(&mut self, ask: Ask) {
        self.ask(Prompt::from_ask(ask));
    }

    /// The palette was hidden or WinCraft is closing: picks not saved yet
    /// are written now.
    pub fn flush_usage(&mut self) {
        self.router.flush_usage();
    }

    pub fn clear_usage(&mut self) {
        self.router.clear_usage();
        self.searched = None;
    }

    /// The host sent a new list of commands.
    pub fn invalidate(&mut self) {
        self.searched = None;
        self.tips_synced = false;
    }

    pub(super) fn sync_tips(&mut self, snapshot: &UiSnapshot) {
        if !std::mem::replace(&mut self.tips_synced, true) {
            self.tips = snapshot.tips.clone();
        }
    }

    pub(super) fn send_tips(&self, to_host: &Arc<HostChannel>) {
        to_host.send(HostRequest::SetHostSetting(HostSetting::Tips(
            self.tips.clone(),
        )));
    }

    fn no_bookmarks(&mut self) -> bool {
        self.bookmarks
            .get_or_insert_with(bookmark_store::shared)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
    }

    /// Whether the selected row opens a folder or a file that Ctrl+B can
    /// bookmark. A web address gets no tip.
    fn selected_place(&mut self) -> Option<Place> {
        let target = self
            .selected_item()?
            .bookmark
            .as_ref()
            .and_then(bookmark_rows::path_of)?
            .to_string();
        if let Some((known, place)) = &self.place {
            if *known == target {
                return Some(*place);
            }
        }
        let place = if Path::new(&target).is_dir() {
            Place::Folder
        } else {
            Place::File
        };
        self.place = Some((target, place));
        Some(place)
    }

    /// What goes over the footer now: the confirmation after a save, or the
    /// Ctrl+B tip on a folder or file row while there are no bookmarks. The
    /// first frame the tip is drawn in counts as this opening's one show.
    pub(super) fn current_strip(&mut self, to_host: &Arc<HostChannel>) -> Option<Strip> {
        if let Some(saved) = &self.saved {
            return Some(Strip::Saved(saved.phrase.clone()));
        }
        let tip = &tips::BOOKMARK_CTRL_B;
        if !self.tips.wanted(tip, self.tip_counted) {
            return None;
        }
        let place = self.selected_place()?;
        if !self.no_bookmarks() {
            return None;
        }
        if !std::mem::replace(&mut self.tip_counted, true) {
            self.tips.record_show(tip);
            self.send_tips(to_host);
        }
        Some(Strip::Hint(place))
    }

    /// A search provider's question is over. A bookmark that was saved keeps
    /// the palette open for a few seconds with a confirmation, and counts as
    /// the use of the feature the folder tip teaches.
    pub(super) fn after_answer(
        &mut self,
        provider: &str,
        typed: &str,
        to_host: &Arc<HostChannel>,
    ) -> Outcome {
        let Some(phrase) = self.saved_phrase(provider, typed) else {
            return Outcome::Hide;
        };
        let mut changed = self.tips.finish(&tips::BOOKMARK_CTRL_B);
        let confirm = self.tips.wanted(&tips::BOOKMARK_SAVED, false);
        if confirm {
            self.tips.record_show(&tips::BOOKMARK_SAVED);
            changed = true;
        }
        if changed {
            self.send_tips(to_host);
        }
        if !confirm {
            return Outcome::Hide;
        }
        self.leave_prompt();
        self.saved = Some(Saved {
            phrase,
            until: Some(Instant::now() + SAVED_FOR),
        });
        Outcome::Stay
    }

    /// The phrase as saved, when the answer just given saved a bookmark.
    fn saved_phrase(&mut self, provider: &str, typed: &str) -> Option<String> {
        if provider != bookmark_rows::PROVIDER {
            return None;
        }
        self.bookmarks
            .get_or_insert_with(bookmark_store::shared)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .find(typed)
            .map(|bookmark| bookmark.phrase.clone())
    }

    /// Back to the plain palette with an empty box, as when it was opened.
    fn leave_prompt(&mut self) {
        self.prompt = None;
        self.select_all = false;
        self.query.clear();
        self.prefix = None;
        self.selected = 0;
        self.follow_selection = true;
        self.searched = None;
    }

    /// Asks the providers again only when the query changed, because some of
    /// them read the disk or the window list, or when one of them has new
    /// background results. While one still waits for some, the palette wakes
    /// up every 100 ms to check, since nothing else would repaint it. When
    /// nothing matches a query typed without a prefix, the rows are the
    /// "Try instead" ones.
    pub(super) fn refresh(&mut self, snapshot: &UiSnapshot, ctx: &egui::Context) {
        let (news, waiting) = self.router.poll();
        let index_news = self.router.take_index_news();
        // The empty view was built before the app index was ready: build it
        // again and keep the user on the row they are on.
        let refill = index_news
            && self.prompt.is_none()
            && self.prefix.is_none()
            && self.query.trim().is_empty();
        let keep = if refill {
            self.selected_item().map(RowId::of)
        } else {
            None
        };
        if news || refill {
            self.searched = None;
        }
        if waiting {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let wanted = (self.prefix.clone(), self.query.clone());
        if self.searched.as_ref() != Some(&wanted) {
            let at_end = self.selected + 1 >= self.results.items.len();
            self.results =
                self.router
                    .search(self.prefix.as_deref(), &self.query, &context(snapshot));
            self.trying = self.results.items.is_empty()
                && self.prefix.is_none()
                && !self.query.trim().is_empty();
            if self.trying {
                self.results.items = actions::try_instead(&self.query, &snapshot.search);
            }
            // Output streaming in keeps the newest line in view, unless the
            // user moved the cursor up to read something.
            if news && at_end && self.results.follows_end {
                self.selected = self.results.items.len().saturating_sub(1);
                self.follow_selection = true;
            }
            if let Some(keep) = keep {
                if let Some(at) = self.results.items.iter().position(|item| keep.is(item)) {
                    if at != self.selected {
                        self.selected = at;
                        self.follow_selection = true;
                    }
                }
            }
            self.searched = Some(wanted);
        }
        if self.selected >= self.results.items.len() {
            self.selected = self.results.items.len().saturating_sub(1);
        }
    }

    pub(super) fn selected_item(&self) -> Option<&ResultItem> {
        self.results.items.get(self.selected)
    }

    /// Puts `text` in the box as if it had been typed, so a prefix at its
    /// start becomes the chip. Runs nothing.
    pub fn type_in(&mut self, text: &str) {
        match self.router.split(text) {
            Some((prefix, rest)) => {
                let rest = rest.to_string();
                self.prefix = Some(prefix);
                self.set_query(rest);
            }
            None => self.set_query(text.to_string()),
        }
    }

    pub(super) fn set_query(&mut self, text: String) {
        self.query = text;
        self.selected = 0;
        self.follow_selection = true;
        self.cursor_to_end = true;
        self.action_cursor = None;
    }

    pub(super) fn set_prefix(&mut self, prefix: Option<String>) {
        self.prefix = prefix;
        self.set_query(String::new());
    }

    /// The selected row's Tab action, when its provider says it can run.
    pub(super) fn tab_action(&mut self) -> Option<Choice> {
        let choice = self.selected_item()?.tab_action.clone()?;
        let Action::Provider { provider, command } = &choice.action else {
            return Some(choice);
        };
        self.router.available(provider, command).then_some(choice)
    }

    /// Tab: the row's Tab action or completion, if it has one. On a row of
    /// the `?` list it switches to that prefix, the same as Enter.
    pub(super) fn complete(&mut self) -> Option<Action> {
        if let Some(choice) = self.tab_action() {
            return Some(choice.action);
        }
        let item = self.selected_item()?;
        if let Some(Choice {
            action: action @ Action::SetPrefix(_),
            ..
        }) = &item.enter
        {
            return Some(action.clone());
        }
        let text = item.tab.as_ref()?.text.clone();
        self.set_query(text);
        None
    }

    /// The selected row is the best match from the picks.
    pub(super) fn on_best_match(&self) -> bool {
        self.results.best_match && self.selected == 0 && !self.trying
    }

    /// What the selected row can do; with windows marked, what the marked
    /// windows can do together.
    pub(super) fn current_actions(&mut self) -> Vec<RowAction> {
        if !self.marks.is_empty() {
            return actions::marks_actions(&self.marks.windows());
        }
        let tab_action = self.tab_action();
        let best_match = self.on_best_match();
        let Some(item) = self.selected_item() else {
            return Vec::new();
        };
        let situation = Situation {
            tab_action,
            best_match,
            marked: item.window().is_some_and(|hwnd| self.marks.contains(hwnd)),
        };
        actions::row_actions(item, &situation)
    }

    /// The text cursor is at the end of the box with nothing selected, so →
    /// is free to open the actions and Ctrl+C to copy a row.
    pub(super) fn text_cursor_at_end(&self, ctx: &egui::Context) -> bool {
        self.text_cursor(ctx).is_none_or(|(primary, secondary)| {
            primary == secondary && primary == self.query.chars().count()
        })
    }

    pub(super) fn nothing_selected_in_box(&self, ctx: &egui::Context) -> bool {
        self.text_cursor(ctx)
            .is_none_or(|(primary, secondary)| primary == secondary)
    }

    fn text_cursor(&self, ctx: &egui::Context) -> Option<(usize, usize)> {
        let state = egui::TextEdit::load_state(ctx, self.query_id?)?;
        let range = state.cursor.char_range()?;
        Some((range.primary.index.into(), range.secondary.index.into()))
    }

    /// The place line of a window, for the marked list.
    pub(super) fn window_place(item: &ResultItem) -> String {
        match item.window() {
            Some(hwnd) => {
                crate::search::start_com();
                window_info::read(hwnd as HWND, None).place_line()
            }
            None => String::new(),
        }
    }

    /// The plugin command on the selected row, if it is one of a plugin
    /// that is on.
    pub(super) fn selected_command(&self) -> Option<CommandId> {
        let item = self.selected_item().filter(|item| !item.disabled)?;
        match item.enter.as_ref().map(|choice| &choice.action) {
            Some(Action::Command(id @ CommandId::Plugin { .. })) => Some(*id),
            _ => None,
        }
    }

    /// Asks the host once what the selected plugin command's plugin has to
    /// say about it, when the cursor gets there.
    pub(super) fn refresh_command_details(&mut self, to_host: &Arc<HostChannel>) {
        let Some(id) = self.selected_command() else {
            return;
        };
        if self
            .command_details
            .as_ref()
            .is_some_and(|(known, _)| *known == id)
        {
            return;
        }
        self.command_details = Some((id, None));
        to_host.send(HostRequest::CommandDetails(id));
    }

    /// The host's answer; an answer for a row the cursor has left is
    /// dropped.
    pub fn set_command_details(&mut self, id: CommandId, details: Option<CommandDetails>) {
        if let Some((known, slot)) = self.command_details.as_mut() {
            if *known == id {
                *slot = details;
            }
        }
    }

    /// The details for the selected row, when they have come.
    pub(super) fn shown_command_details(&self) -> Option<&CommandDetails> {
        let id = self.selected_command()?;
        self.command_details
            .as_ref()
            .filter(|(known, _)| *known == id)
            .and_then(|(_, details)| details.as_ref())
    }

    /// What the selected command says before its second Enter, while it
    /// waits for one.
    pub(super) fn armed_text(&self, snapshot: &UiSnapshot) -> Option<String> {
        let (id, at) = self.armed?;
        if self.selected_command() != Some(id) || at.elapsed() >= CONFIRM_FOR {
            return None;
        }
        confirm_text(snapshot, id)
    }

    /// Enter on a command that asks for it twice: true when this is the
    /// second Enter within 5 s and the command may run; false arms it.
    pub(super) fn confirmed(&mut self, snapshot: &UiSnapshot, id: CommandId) -> bool {
        if confirm_text(snapshot, id).is_none() {
            return true;
        }
        if self
            .armed
            .is_some_and(|(armed, at)| armed == id && at.elapsed() < CONFIRM_FOR)
        {
            self.armed = None;
            return true;
        }
        self.armed = Some((id, Instant::now()));
        false
    }

    /// Reads the selected window's details once, when the cursor gets there.
    pub(super) fn refresh_details(&mut self) {
        let Some(hwnd) = self.selected_item().and_then(ResultItem::window) else {
            return;
        };
        if self
            .details
            .as_ref()
            .is_some_and(|(known, _)| *known == hwnd)
        {
            return;
        }
        crate::search::start_com();
        let pinner = self
            .pinner
            .get_or_insert_with(|| match Pinner::new() {
                Ok(pinner) => Some(pinner),
                Err(err) => {
                    log::info!("palette: pinning windows to all desktops is not available: {err}");
                    None
                }
            })
            .as_ref();
        let details = window_info::read(hwnd as HWND, pinner);
        self.details = Some((hwnd, details));
    }

    /// The inspector's last line: where the row comes from.
    pub(super) fn source_line(&self, item: &ResultItem, snapshot: &UiSnapshot) -> String {
        if self.trying || item.provider.is_empty() {
            return String::new();
        }
        match item.provider.as_str() {
            "windows" => {
                let program = self
                    .details
                    .as_ref()
                    .filter(|(hwnd, _)| Some(*hwnd) == item.window())
                    .map(|(_, details)| details.process.clone())
                    .filter(|process| !process.is_empty())
                    .unwrap_or_else(|| {
                        item.subtitle
                            .split(" \u{b7} ")
                            .next()
                            .unwrap_or_default()
                            .to_string()
                    });
                format!("Open window \u{b7} {program}")
            }
            "calc" => "Built-in calculator".to_string(),
            "commands" => match &item.origin {
                Some(plugin) => snapshot
                    .plugins
                    .iter()
                    .find(|info| info.id == *plugin)
                    .map(|info| format!("From {} {}", info.name, info.version))
                    .unwrap_or_else(|| "From a plugin".to_string()),
                None => format!("WinCraft {}", snapshot.version),
            },
            "apps" => "App from the Start menu".to_string(),
            "files" => "File search".to_string(),
            "paths" => "Folders and files".to_string(),
            "bookmarks" => "Your bookmark".to_string(),
            "terminal" => "Terminal".to_string(),
            "web" => "Ask AI and web search".to_string(),
            other => self
                .router
                .provider_name(other)
                .map(|name| format!("From {name}"))
                .unwrap_or_default(),
        }
    }
}

/// What tells a row from the others when the list is built again.
struct RowId {
    provider: String,
    key: Option<String>,
    title: String,
    window: Option<isize>,
}

impl RowId {
    fn of(item: &ResultItem) -> Self {
        Self {
            provider: item.provider.clone(),
            key: item.usage_key.clone(),
            title: item.title.clone(),
            window: item.window(),
        }
    }

    fn is(&self, item: &ResultItem) -> bool {
        self.provider == item.provider
            && self.key == item.usage_key
            && self.title == item.title
            && self.window == item.window()
    }
}

/// How long a first Enter on a command that asks for two stays waiting.
pub(super) const CONFIRM_FOR: Duration = Duration::from_secs(5);

/// The text a command shows before its second Enter, if it asks for one.
pub(super) fn confirm_text(snapshot: &UiSnapshot, id: CommandId) -> Option<String> {
    snapshot
        .commands
        .iter()
        .find(|entry| entry.id == id && !entry.confirm.is_empty())
        .map(|entry| entry.confirm.clone())
}
