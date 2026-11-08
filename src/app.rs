use crate::model::{Project, Related, Review, Status};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq)]
pub enum Focus {
    Outline,
    Code,
}

#[derive(Clone)]
pub struct View {
    pub selected: usize,
    pub file: String,
    pub target_line: usize,
    pub scroll: usize,
    pub horizontal: u16,
    pub expanded: bool,
    pub focus: Focus,
    pub evidence: bool,
    pub overview: bool,
}

pub enum Popup {
    Help,
    Related(usize),
    Comments(usize),
}

pub struct CommentDraft {
    pub overview: bool,
    pub annotation: usize,
    pub text: String,
    pub cursor: usize, // UTF-8 byte boundary, independent of terminal cell width.
    pub editing: bool,
    pub edit_index: Option<usize>,
    previous_focus: Focus,
}

pub struct App {
    pub project: Project,
    pub highlights: crate::highlight::SourceCache,
    pub review: Review,
    pub view: View,
    pub history: Vec<View>,
    pub popup: Option<Popup>,
    pub comment: Option<CommentDraft>,
    pub recenter: bool,
    pub notice: Option<String>,
    pub dirty: bool,
    pub quit: bool,
    pub page_height: usize,
    pub row_count: usize,
    pub popup_scroll: u16,
}

impl App {
    pub fn new(project: Project) -> Self {
        let after = project
            .diffs
            .iter()
            .map(|(path, diff)| {
                (
                    path.clone(),
                    diff.after
                        .as_deref()
                        .unwrap_or("")
                        .lines()
                        .map(|line| line.replace('\t', "    "))
                        .collect(),
                )
            })
            .collect();
        let highlights = crate::highlight::SourceCache::new(&project.files, &after);
        let file = project.proposal.annotations[0].file.clone();
        let target_line = project.proposal.annotations[0].line;
        let (review, message) = project.read_review();
        let notice = (review.stale || review.load_failed).then_some(message);
        Self {
            project,
            highlights,
            review,
            view: View {
                selected: 0,
                file,
                target_line,
                scroll: 0,
                horizontal: 0,
                expanded: false,
                focus: Focus::Outline,
                evidence: false,
                overview: true,
            },
            history: vec![],
            popup: None,
            comment: None,
            recenter: true,
            notice,
            dirty: false,
            quit: false,
            page_height: 10,
            row_count: 0,
            popup_scroll: 0,
        }
    }

    pub fn notify(&mut self, message: String) {
        self.notice = Some(message);
    }

    pub fn select(&mut self, index: usize, remember: bool) {
        if remember || self.view.overview {
            self.history.push(self.view.clone());
        }
        let note = &self.project.proposal.annotations[index];
        self.view.selected = index;
        self.view.file = note.file.clone();
        self.view.target_line = note.line;
        self.view.scroll = 0;
        self.view.horizontal = 0;
        self.view.evidence = false;
        self.view.overview = false;
        self.recenter = true;
    }

    pub fn show_overview(&mut self) {
        if !self.view.overview {
            if let Some(view) = self.history.iter().rev().find(|view| view.overview) {
                self.view = view.clone();
            } else {
                self.view.overview = true;
                self.view.evidence = false;
                self.view.scroll = 0;
            }
        }
        self.history.clear();
        self.view.focus = Focus::Outline;
        self.recenter = false;
    }

    fn step_outline(&mut self, delta: isize) {
        let count = self.project.proposal.annotations.len() + 1;
        let current = if self.view.overview {
            0
        } else {
            self.view.selected + 1
        };
        let next = (current as isize + delta).rem_euclid(count as isize) as usize;
        if next == 0 {
            self.show_overview();
        } else {
            self.select(next - 1, false);
        }
    }

    fn step_overview_link(&mut self, delta: isize) {
        let count = self.project.proposal.annotations.len();
        self.view.selected =
            (self.view.selected as isize + delta).rem_euclid(count as isize) as usize;
        self.recenter = true;
    }

    fn step(&mut self, delta: isize) {
        let count = self.project.proposal.annotations.len();
        if self.view.overview {
            self.select(if delta > 0 { 0 } else { count - 1 }, true);
            return;
        }
        let index = (self.view.selected as isize + delta).rem_euclid(count as isize) as usize;
        self.select(index, false);
    }

    pub fn follow(&mut self, index: usize) {
        let link = &self.project.proposal.annotations[self.view.selected].related[index];
        match link {
            Related::Annotation { annotation, .. } => self.select(*annotation, true),
            Related::Code { file, line, .. } => {
                self.history.push(self.view.clone());
                self.view.file = file.clone();
                self.view.target_line = *line;
                self.view.scroll = 0;
                self.view.horizontal = 0;
                self.view.focus = Focus::Code;
                self.view.evidence = true;
                self.recenter = true;
            }
        }
    }

    fn scroll(&mut self, delta: isize) {
        self.view.scroll = self
            .view
            .scroll
            .saturating_add_signed(delta)
            .min(self.row_count.saturating_sub(self.page_height));
        self.recenter = false;
    }

    pub fn save(&mut self) {
        match self.project.save_review(&self.review) {
            Ok(()) => {
                self.review = self.project.read_review().0;
                self.dirty = false;
                self.notify(format!(
                    "Saved feedback to {} · Source unchanged",
                    self.project
                        .state_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ));
            }
            Err(e) => self.notify(format!("Save failed: {e}")),
        }
    }

    pub fn reload(&mut self) {
        if self.dirty || self.comment.is_some() {
            self.notify(
                "Reload paused · save feedback with s and finish or cancel your draft first".into(),
            );
            return;
        }
        let project = match self.project.reload() {
            Ok(project) => project,
            Err(error) => {
                self.notify(format!("Reload failed · current view retained: {error}"));
                return;
            }
        };
        let selected_id = &self.project.proposal.annotations[self.view.selected].id;
        let selected = project
            .proposal
            .annotations
            .iter()
            .position(|note| &note.id == selected_id);
        let mut fresh = Self::new(project);
        if let Some(index) = selected {
            fresh.select(index, false);
            fresh.view.expanded = self.view.expanded;
            fresh.view.overview = self.view.overview;
            fresh.view.focus = self.view.focus;
            fresh.history.clear();
        }
        fresh.notify(format!(
            "Reloaded proposal and source · {}",
            fresh.notice.as_deref().unwrap_or("review ready")
        ));
        *self = fresh;
    }

    /// Import feedback from another viewer while idle, without replacing an unsaved human draft.
    pub fn refresh_review(&mut self) {
        if self.project.assert_current().is_err() {
            self.review.stale = true;
            self.review.approved = false;
            self.notify(
                "Code or proposal changed · previous approvals stale · R reloads code".into(),
            );
            return;
        }
        let (fresh, message) = self.project.read_review();
        if fresh.generation != self.review.generation
            || fresh.stale != self.review.stale
            || fresh.load_failed
        {
            if self.dirty || self.comment.is_some() {
                self.review.approved = false;
                self.notify(
                    "Shared review changed · local draft retained · save will check for conflicts"
                        .into(),
                );
            } else {
                self.notice = (fresh.stale || fresh.load_failed).then_some(message);
                self.review = fresh;
            }
        }
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(draft) = self.comment.as_mut().filter(|draft| draft.editing) {
            let text: String = text
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .replace('\t', "    ")
                .chars()
                .filter(|c| *c == '\n' || !c.is_control())
                .collect();
            draft.text.insert_str(draft.cursor, &text);
            draft.cursor += text.len();
        }
    }

    pub fn key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.popup.is_some() {
            self.popup_key(key.code);
            return;
        }
        if self.comment.is_some() {
            self.comment_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('q') => {
                if self.dirty {
                    self.save();
                    if self.dirty {
                        return;
                    }
                }
                self.quit = true;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.view.focus = if self.view.focus == Focus::Outline {
                    Focus::Code
                } else {
                    Focus::Outline
                };
                if self.view.overview && self.view.focus == Focus::Code {
                    self.recenter = true;
                }
            }
            KeyCode::Char('n') => self.step(1),
            KeyCode::Char('p') => self.step(-1),
            KeyCode::Down | KeyCode::Char('j') => {
                if self.view.focus == Focus::Outline {
                    self.step_outline(1);
                } else if self.view.overview {
                    self.step_overview_link(1);
                } else {
                    self.scroll(1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.view.focus == Focus::Outline {
                    self.step_outline(-1);
                } else if self.view.overview {
                    self.step_overview_link(-1);
                } else {
                    self.scroll(-1);
                }
            }
            KeyCode::PageDown => self.scroll(self.page_height as isize),
            KeyCode::PageUp => self.scroll(-(self.page_height as isize)),
            KeyCode::Left | KeyCode::Char('h') => {
                self.view.horizontal = self.view.horizontal.saturating_sub(8)
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.view.horizontal = self.view.horizontal.saturating_add(8)
            }
            KeyCode::Home => {
                self.view.scroll = 0;
                self.recenter = false;
            }
            KeyCode::End => {
                self.view.scroll = self.row_count.saturating_sub(self.page_height);
                self.recenter = false;
            }
            KeyCode::Enter | KeyCode::Char('w') | KeyCode::Char(' ') => {
                if self.view.overview {
                    if key.code == KeyCode::Enter {
                        self.select(self.view.selected, true);
                    }
                    return;
                }
                self.view.expanded = !self.view.expanded;
                let index = self.view.selected;
                self.select(index, false);
            }
            KeyCode::Char('r') => {
                if self.view.overview {
                    return;
                }
                if !self.project.proposal.annotations[self.view.selected]
                    .related
                    .is_empty()
                {
                    self.popup = Some(Popup::Related(0));
                }
            }
            KeyCode::Esc | KeyCode::Backspace => {
                if let Some(view) = self.history.pop() {
                    self.view = view;
                    self.recenter = false;
                } else {
                    self.show_overview();
                }
            }
            KeyCode::Char('v') | KeyCode::Char('f') => {
                if self.view.overview {
                    return;
                }
                let desired = if key.code == KeyCode::Char('v') {
                    Status::Reviewed
                } else {
                    Status::Flagged
                };
                let note = &mut self.review.notes[self.view.selected];
                note.status = if note.status == desired {
                    Status::Pending
                } else {
                    desired
                };
                self.review.approved = false;
                self.review.approval_changed = true;
                self.dirty = true;
            }
            KeyCode::Char('e') => {
                if self.current_comments().is_empty() {
                    self.notify("No comments here yet · c adds a comment".into());
                } else {
                    self.popup = Some(Popup::Comments(0));
                }
            }
            KeyCode::Char('c') => {
                self.comment = Some(CommentDraft {
                    overview: self.view.overview,
                    annotation: self.view.selected,
                    text: String::new(),
                    cursor: 0,
                    editing: true,
                    edit_index: None,
                    previous_focus: self.view.focus,
                });
                self.view.focus = Focus::Code;
                self.recenter = false;
            }
            KeyCode::Char('a') => {
                self.review.approved = !self.review.approved;
                self.review.approval_changed = true;
                self.dirty = true;
                self.notify(
                    "Approval is local review metadata; no implementation is triggered".into(),
                );
            }
            KeyCode::Char('s') => self.save(),
            KeyCode::Char('R') => self.reload(),
            KeyCode::Char('?') => {
                self.popup_scroll = 0;
                self.popup = Some(Popup::Help);
            }
            KeyCode::Char('o') => {
                self.show_overview();
            }
            _ => {}
        }
    }

    fn comment_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                let draft = self.comment.take().unwrap();
                self.view.focus = draft.previous_focus;
                self.notify("Comment canceled".into());
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let draft = self.comment.as_mut().unwrap();
                draft.editing = !draft.editing;
            }
            KeyCode::PageUp => self.scroll(-(self.page_height as isize)),
            KeyCode::PageDown => self.scroll(self.page_height as isize),
            KeyCode::Up => self.scroll(-1),
            KeyCode::Down => self.scroll(1),
            _ if self.comment.as_ref().unwrap().editing => {
                if key.code == KeyCode::Enter {
                    if self.comment.as_ref().unwrap().text.trim().is_empty() {
                        return;
                    }
                    let draft = self.comment.take().unwrap();
                    let body = draft.text.trim().to_owned();
                    if let Some(index) = draft.edit_index {
                        let old = if draft.overview {
                            &self.review.overview_comments[index]
                        } else {
                            &self.review.notes[draft.annotation].comments[index]
                        };
                        let mut display = body.clone();
                        if let Some(source) = self.review.comment_sources.get(old).cloned() {
                            if source.author != "human" {
                                self.notify("Only your own comments can be edited".into());
                                return;
                            }
                            self.review
                                .comment_edits
                                .insert(source.id.clone(), body.clone());
                            let mut changed = source.clone();
                            changed.body = body;
                            display = crate::review::display_comment(&changed);
                            self.review.comment_sources.insert(display.clone(), source);
                        }
                        if draft.overview {
                            self.review.overview_comments[index] = display;
                        } else {
                            self.review.notes[draft.annotation].comments[index] = display;
                        }
                    } else if draft.overview {
                        self.review.overview_comments.push(body);
                    } else {
                        self.review.notes[draft.annotation].comments.push(body);
                    }
                    self.review.approved = false;
                    self.review.approval_changed = true;
                    self.dirty = true;
                    self.view.focus = draft.previous_focus;
                    self.notify(
                        if draft.edit_index.is_some() {
                            "Comment updated · s saves feedback"
                        } else {
                            "Comment added · s saves feedback"
                        }
                        .into(),
                    );
                    return;
                }
                let draft = self.comment.as_mut().unwrap();
                match key.code {
                    KeyCode::Char(c)
                        if !c.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        draft.text.insert(draft.cursor, c);
                        draft.cursor += c.len_utf8();
                    }
                    KeyCode::Left => {
                        draft.cursor = draft.text[..draft.cursor]
                            .char_indices()
                            .next_back()
                            .map_or(0, |(i, _)| i);
                    }
                    KeyCode::Right => {
                        if let Some(c) = draft.text[draft.cursor..].chars().next() {
                            draft.cursor += c.len_utf8();
                        }
                    }
                    KeyCode::Home => draft.cursor = 0,
                    KeyCode::End => draft.cursor = draft.text.len(),
                    KeyCode::Backspace if draft.cursor > 0 => {
                        let previous = draft.text[..draft.cursor]
                            .char_indices()
                            .next_back()
                            .unwrap()
                            .0;
                        draft.text.drain(previous..draft.cursor);
                        draft.cursor = previous;
                    }
                    KeyCode::Delete => {
                        if let Some(c) = draft.text[draft.cursor..].chars().next() {
                            draft.text.drain(draft.cursor..draft.cursor + c.len_utf8());
                        }
                    }
                    _ => {}
                }
            }
            _ => match key.code {
                KeyCode::Char('j') => self.scroll(1),
                KeyCode::Char('k') => self.scroll(-1),
                KeyCode::Char('h') | KeyCode::Left => {
                    self.view.horizontal = self.view.horizontal.saturating_sub(8)
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    self.view.horizontal = self.view.horizontal.saturating_add(8)
                }
                KeyCode::Home => {
                    self.view.scroll = 0;
                    self.recenter = false;
                }
                KeyCode::End => self.scroll(self.row_count as isize),
                KeyCode::Char('w') | KeyCode::Char(' ') => {
                    self.view.expanded = !self.view.expanded;
                    self.select(self.view.selected, false);
                }
                KeyCode::Enter | KeyCode::Char('c') => {
                    self.comment.as_mut().unwrap().editing = true
                }
                _ => {}
            },
        }
    }

    pub fn current_comments(&self) -> &[String] {
        if self.view.overview {
            &self.review.overview_comments
        } else {
            &self.review.notes[self.view.selected].comments
        }
    }

    pub fn comment_body(&self, index: usize) -> &str {
        let display = &self.current_comments()[index];
        if let Some(source) = self.review.comment_sources.get(display) {
            self.review
                .comment_edits
                .get(&source.id)
                .unwrap_or(&source.body)
        } else {
            display
        }
    }

    pub fn comment_editable(&self, index: usize) -> bool {
        self.current_comments().get(index).is_some_and(|text| {
            self.review
                .comment_sources
                .get(text)
                .is_none_or(|source| source.author == "human")
        })
    }

    fn edit_comment(&mut self, index: usize) {
        if !self.comment_editable(index) {
            self.notify("Only your own comments can be edited".into());
            self.popup = None;
            return;
        }
        let text = self.comment_body(index).to_owned();
        self.comment = Some(CommentDraft {
            overview: self.view.overview,
            annotation: self.view.selected,
            cursor: text.len(),
            text,
            editing: true,
            edit_index: Some(index),
            previous_focus: self.view.focus,
        });
        self.popup = None;
        self.view.focus = Focus::Code;
        self.recenter = false;
    }

    fn popup_key(&mut self, key: KeyCode) {
        if key == KeyCode::Esc {
            self.popup = None;
            return;
        }
        if let Some(Popup::Comments(selected)) = self.popup.as_ref() {
            let count = self.current_comments().len();
            if count == 0 {
                self.popup = None;
                return;
            }
            let selected = (*selected).min(count - 1);
            match key {
                KeyCode::Down | KeyCode::Char('j') => {
                    self.popup = Some(Popup::Comments((selected + 1) % count))
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.popup = Some(Popup::Comments((selected + count - 1) % count))
                }
                KeyCode::Enter | KeyCode::Char('e') => self.edit_comment(selected),
                _ => {}
            }
            return;
        }
        match self.popup.as_mut().unwrap() {
            Popup::Related(selected) => {
                let count = self.project.proposal.annotations[self.view.selected]
                    .related
                    .len();
                match key {
                    KeyCode::Down | KeyCode::Char('j') => *selected = (*selected + 1) % count,
                    KeyCode::Up | KeyCode::Char('k') => *selected = (*selected + count - 1) % count,
                    KeyCode::Enter => {
                        let index = *selected;
                        self.popup = None;
                        self.follow(index);
                    }
                    _ => {}
                }
            }
            _ => match key {
                KeyCode::Down | KeyCode::Char('j') => {
                    self.popup_scroll = self.popup_scroll.saturating_add(1)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.popup_scroll = self.popup_scroll.saturating_sub(1)
                }
                KeyCode::Enter | KeyCode::Char('q') => self.popup = None,
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn app() -> App {
        App::new(
            Project::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/proposal.json"))
                .unwrap(),
        )
    }

    #[test]
    fn reload_tracks_annotation_ids_and_retains_view_on_invalid_proposal() {
        let mut app = app();
        let path =
            std::env::temp_dir().join(format!("patchwalk-reload-{}.json", std::process::id()));
        let mut proposal = app.project.wire_proposal.clone();
        std::fs::write(&path, serde_json::to_vec(&proposal).unwrap()).unwrap();
        app = App::new(
            Project::load_with_root(
                &path,
                Some(&Path::new(env!("CARGO_MANIFEST_DIR")).join("demo")),
            )
            .unwrap(),
        );
        app.select(1, false);
        let selected_id = app.project.proposal.annotations[1].id.clone();
        app.view.expanded = true;
        app.follow(1);
        proposal["annotations"].as_array_mut().unwrap().swap(0, 1);
        proposal["revision"] = 2.into();
        std::fs::write(&path, serde_json::to_vec(&proposal).unwrap()).unwrap();
        app.key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT));
        assert_eq!(app.project.proposal.revision, 2);
        assert_eq!(app.view.selected, 0);
        assert_eq!(app.project.proposal.annotations[0].id, selected_id);
        assert!(!app.view.evidence);
        assert!(app.view.expanded);
        assert!(app.history.is_empty());

        std::fs::write(&path, "invalid JSON").unwrap();
        app.reload();
        assert!(
            app.notice
                .as_deref()
                .unwrap_or("")
                .starts_with("Reload failed")
        );
        assert_eq!(app.project.proposal.revision, 2);
        assert_eq!(app.view.selected, 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reload_protects_unsaved_feedback_and_comment_drafts() {
        let mut app = app();
        app.select(1, false);
        app.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        app.reload();
        assert!(app.dirty);
        assert_eq!(app.review.notes[1].status, Status::Flagged);
        assert!(
            app.notice
                .as_deref()
                .unwrap_or("")
                .starts_with("Reload paused")
        );

        app.dirty = false;
        app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        app.paste("Keep this draft");
        app.reload();
        assert_eq!(app.comment.as_ref().unwrap().text, "Keep this draft");
        assert!(
            app.notice
                .as_deref()
                .unwrap_or("")
                .starts_with("Reload paused")
        );
    }

    #[test]
    fn evidence_back_restores_scroll_focus_and_expansion() {
        let mut app = app();
        app.select(1, false);
        app.view.scroll = 7;
        app.view.expanded = true;
        app.view.focus = Focus::Outline;
        app.follow(1);
        assert_eq!(app.view.file, "src/returns/service.ts");
        assert!(app.view.evidence);
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.view.file, "src/orders/service.ts");
        assert_eq!(app.view.scroll, 7);
        assert!(app.view.expanded);
        assert!(app.view.focus == Focus::Outline);
        assert!(!app.view.evidence);
    }

    #[test]
    fn annotations_wrap_and_related_notes_support_back_navigation() {
        let mut app = app();
        app.step(-1);
        assert_eq!(app.view.selected, 4);
        app.step(1);
        assert_eq!(app.view.selected, 0);
        app.follow(1);
        assert_eq!(app.view.selected, 1);
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.view.selected, 0);
    }

    #[test]
    fn local_comments_and_status_changes_invalidate_approval() {
        let mut app = app();
        app.select(0, false);
        app.review.approved = true;
        app.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(!app.review.approved);
        assert_eq!(app.review.notes[0].status, Status::Flagged);
        app.review.approved = true;
        app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        for c in "Why here?".chars() {
            app.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.review.notes[0].comments.last().unwrap(), "Why here?");
        assert!(!app.review.approved);
    }

    #[test]
    fn overview_is_home_and_nested_exploration_returns_to_it() {
        let mut app = app();
        assert!(app.view.overview);
        app.view.scroll = 3;
        app.view.focus = Focus::Code;
        app.step_overview_link(1);
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.view.overview);
        assert_eq!(app.view.selected, 1);
        app.follow(1);
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.view.overview);
        assert!(!app.view.evidence);
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.view.overview);
        assert_eq!(app.view.selected, 1);
        assert_eq!(app.view.scroll, 3);
        assert!(app.view.focus == Focus::Code);
    }

    #[test]
    fn sidebar_includes_overview_and_shortcut_returns_directly_home() {
        let mut app = app();
        app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(!app.view.overview);
        assert_eq!(app.view.selected, 0);
        app.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert!(app.view.overview);
        app.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.view.selected, 4);
        app.follow(0);
        assert!(app.view.evidence);
        app.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        assert!(app.view.overview);
        assert!(!app.view.evidence);
        assert!(app.history.is_empty());
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.view.overview);
    }

    #[test]
    fn annotation_review_commands_do_not_affect_hidden_notes_on_overview() {
        let mut app = app();
        let initial = app.review.notes[0].status;
        for command in ['v', 'f', 'r', 'w'] {
            app.key(KeyEvent::new(KeyCode::Char(command), KeyModifiers::NONE));
        }
        assert!(app.view.overview);
        assert!(app.popup.is_none());
        assert!(!app.dirty);
        assert_eq!(app.review.notes[0].status, initial);
    }
    #[test]
    fn comments_stay_in_context_and_capture_navigation_letters_as_text() {
        let mut app = app();
        app.select(1, false);
        app.view.scroll = 4;
        app.view.expanded = true;
        let initial = app.review.notes[1].comments.len();
        app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(app.popup.is_none());
        assert_eq!(app.view.scroll, 4);
        assert!(app.view.expanded);
        app.paste("Should q, n, and v be accepted here?");
        for c in " qnv".chars() {
            app.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert!(!app.quit);
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.row_count = 100;
        app.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.view.selected, 1);
        assert!(app.view.scroll > 4);
        assert_eq!(app.comment.as_ref().unwrap().annotation, 1);
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.comment.is_none());
        assert_eq!(app.review.notes[1].comments.len(), initial + 1);
        assert_eq!(
            app.review.notes[1].comments.last().unwrap(),
            "Should q, n, and v be accepted here? qnv"
        );
        assert!(app.view.expanded);
        assert!(app.dirty);
    }

    #[test]
    fn empty_submit_and_cancel_do_not_modify_review() {
        let mut app = app();
        app.select(0, false);
        app.review.approved = true;
        let initial = app.review.notes[0].comments.len();
        app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.comment.is_some());
        app.paste("An unfinished thought");
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.comment.is_none());
        assert_eq!(app.review.notes[0].comments.len(), initial);
        assert!(!app.dirty);
        assert!(app.review.approved);
        assert!(app.view.focus == Focus::Outline);
    }

    #[test]
    fn inline_editing_respects_unicode_boundaries_and_multiline_paste() {
        let mut app = app();
        app.select(0, false);
        app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        app.paste("a界🙂");
        for key in [
            KeyCode::Left,
            KeyCode::Backspace,
            KeyCode::Char('é'),
            KeyCode::Delete,
            KeyCode::Home,
            KeyCode::Char('X'),
            KeyCode::End,
            KeyCode::Char('!'),
        ] {
            app.key(KeyEvent::new(key, KeyModifiers::NONE));
        }
        assert_eq!(app.comment.as_ref().unwrap().text, "Xaé!");
        app.paste("\r\nsecond line");
        assert!(!app.dirty); // Paste does not submit or trigger application commands.
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.review.notes[0].comments.last().unwrap(),
            "Xaé!\nsecond line"
        );
    }
}
