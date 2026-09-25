//! The jot window: tabs in the title bar, editor with line numbers, find bar, status bar,
//! focus mode and the keybinds overlay.

use crate::config::{Config, TabBar, MAX_FONT_SIZE, MIN_FONT_SIZE, MIN_OPACITY};
use crate::doc::{hash_of, Doc, GalleyCache};
use crate::highlight::{self, Options, BG, BLUE, BLUE_BRIGHT, FAINT, FG, MUTED, RAISED, RED, YELLOW};
use crate::{fonts, text_util};
use eframe::egui::{
    self, text::CCursor, text::CCursorRange, text_edit::TextEditState, Align, Align2, Color32, FontFamily,
    FontId, Frame, Id, Key, Margin, Modifiers, ScrollArea, TextEdit, Ui, UiBuilder, ViewportCommand,
};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Above this size Markdown styling is skipped to keep editing fast.
const LARGE_FILE_BYTES: usize = 1_000_000;
const TITLE_BAR_HEIGHT: f32 = 30.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const CFG_POLL: Duration = Duration::from_millis(1000);
const CFG_SAVE_DELAY: Duration = Duration::from_millis(600);

/// Every shortcut, shown by the keybinds overlay (`F1`).
const KEYS: &[(&str, &str)] = &[
    ("Ctrl+T / Ctrl+N", "new tab"),
    ("Ctrl+W", "close tab"),
    ("Ctrl+Tab / Ctrl+Shift+Tab", "next / previous tab"),
    ("Alt+1 .. Alt+9", "go to tab"),
    ("Ctrl+O", "open file(s) in tabs"),
    ("Ctrl+S / Ctrl+Shift+S", "save / save as"),
    ("Ctrl+F", "find"),
    ("F3 / Shift+F3", "next / previous match"),
    ("Alt+Z", "toggle word wrap"),
    ("Ctrl+Shift+F", "focus mode"),
    ("F11", "fullscreen"),
    ("Ctrl+Scroll / Ctrl+= / Ctrl+-", "font size"),
    ("Ctrl+0", "reset font size"),
    ("Ctrl+Alt+Up / Down", "opacity"),
    ("Ctrl+Shift+L", "line numbers"),
    ("Ctrl+Shift+M", "markdown styling"),
    ("Ctrl+Shift+B", "tab bar: auto / always / never"),
    ("F1", "this help"),
    ("Esc", "close panels, leave focus mode"),
];

#[derive(Default)]
struct Find {
    open: bool,
    query: String,
    focus_input: bool,
    matches: Vec<Range<usize>>,
    matches_key: u64,
    current: usize,
}

enum TabAction {
    Activate(usize),
    Close(usize),
    New,
}

pub struct Jot {
    cfg: Config,
    cfg_mtime: Option<SystemTime>,
    cfg_polled: Instant,
    cfg_dirty_since: Option<Instant>,

    docs: Vec<Doc>,
    active: usize,
    next_doc_id: u64,

    focus: bool,
    fullscreen: bool,
    help_open: bool,
    force_close: bool,
    focus_editor: bool,

    find: Find,
    status: Option<(String, Instant)>,
    window_title: String,
    find_id: Id,
}

impl Jot {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        let cfg = Config::load();
        let ctx = &cc.egui_ctx;
        let font_found = fonts::install(ctx, &cfg.font);
        configure_visuals(ctx);

        let mut app = Self {
            cfg_mtime: Config::modified(),
            cfg_polled: Instant::now(),
            cfg_dirty_since: None,
            cfg,
            docs: vec![Doc::new(0)],
            active: 0,
            next_doc_id: 1,
            focus: false,
            fullscreen: false,
            help_open: false,
            force_close: false,
            focus_editor: true,
            find: Find::default(),
            status: None,
            window_title: String::new(),
            find_id: Id::new("jot-find"),
        };
        for path in files {
            app.open_path(path);
        }
        if !font_found {
            app.notify(format!("Font \"{}\" not found, using the default", app.cfg.font));
        }
        app
    }

    // ---- state helpers ----------------------------------------------------

    fn doc(&self) -> &Doc {
        &self.docs[self.active]
    }

    fn doc_mut(&mut self) -> &mut Doc {
        &mut self.docs[self.active]
    }

    fn show_tabs(&self) -> bool {
        match self.cfg.tab_bar {
            TabBar::Always => true,
            TabBar::Never => false,
            TabBar::Auto => self.docs.len() > 1,
        }
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn touch_config(&mut self) {
        self.cfg_dirty_since = Some(Instant::now());
    }

    fn new_doc_id(&mut self) -> u64 {
        self.next_doc_id += 1;
        self.next_doc_id
    }

    // ---- tabs -----------------------------------------------------------------

    fn activate(&mut self, i: usize) {
        if i < self.docs.len() && i != self.active {
            self.active = i;
            self.focus_editor = true;
            self.find.matches_key = 0;
        }
    }

    fn new_tab(&mut self) {
        let id = self.new_doc_id();
        self.docs.push(Doc::new(id));
        self.active = self.docs.len() - 1;
        self.focus_editor = true;
        self.find.matches_key = 0;
    }

    fn close_tab(&mut self, i: usize) {
        if i >= self.docs.len() || !self.confirm_discard(i) {
            return;
        }
        if self.docs.len() == 1 {
            // Closing the last tab leaves a fresh empty document instead of quitting.
            let id = self.new_doc_id();
            self.docs[0] = Doc::new(id);
        } else {
            self.docs.remove(i);
            if i < self.active {
                self.active -= 1;
            }
            self.active = self.active.min(self.docs.len() - 1);
        }
        self.focus_editor = true;
        self.find.matches_key = 0;
    }

    fn step_tab(&mut self, forward: bool) {
        let n = self.docs.len();
        if n > 1 {
            let next = if forward { (self.active + 1) % n } else { (self.active + n - 1) % n };
            self.activate(next);
        }
    }

    // ---- files ------------------------------------------------------------

    fn open_path(&mut self, path: PathBuf) {
        if let Some(i) = self.docs.iter().position(|d| d.path.as_ref() == Some(&path)) {
            self.activate(i);
            return;
        }
        let id = self.new_doc_id();
        let (doc, lossy) = match std::fs::read(&path) {
            Ok(bytes) => Doc::from_bytes(id, Some(path), &bytes),
            // `jot new-file.md` starts an empty document that will be created on save.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut d = Doc::new(id);
                d.path = Some(path);
                (d, false)
            }
            Err(e) => {
                self.notify(format!("Could not open file: {e}"));
                return;
            }
        };
        let large = doc.text.len() > LARGE_FILE_BYTES;
        if self.doc().is_blank() && !self.doc().dirty() {
            let slot = self.active;
            self.docs[slot] = doc;
        } else {
            self.docs.push(doc);
            self.active = self.docs.len() - 1;
        }
        self.focus_editor = true;
        self.find.matches_key = 0;
        if lossy {
            self.notify("Invalid UTF-8 found, some characters were replaced");
        } else if large {
            self.notify("Large file: Markdown styling is off to keep editing fast");
        }
    }

    fn open_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new().pick_files() {
            for path in paths {
                self.open_path(path);
            }
        }
    }

    fn save(&mut self) -> bool {
        let Some(path) = self.doc().path.clone().or_else(|| self.pick_save_path()) else {
            return false;
        };
        self.write_to(path)
    }

    fn save_as(&mut self) -> bool {
        match self.pick_save_path() {
            Some(path) => self.write_to(path),
            None => false,
        }
    }

    fn pick_save_path(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name(self.doc().file_name())
            .add_filter("Markdown", &["md", "markdown"])
            .add_filter("Text", &["txt"])
            .add_filter("All files", &["*"])
            .save_file()
    }

    fn write_to(&mut self, path: PathBuf) -> bool {
        let bytes = text_util::encode(&self.doc().text, self.doc().crlf);
        match std::fs::write(&path, bytes) {
            Ok(()) => {
                let doc = self.doc_mut();
                doc.saved_hash = doc.text_hash;
                doc.path = Some(path);
                self.notify("Saved");
                true
            }
            Err(e) => {
                self.notify(format!("Could not save: {e}"));
                false
            }
        }
    }

    /// Asks what to do with unsaved changes in tab `i`. Returns `true` when it is fine to continue.
    fn confirm_discard(&mut self, i: usize) -> bool {
        if !self.docs[i].dirty() {
            return true;
        }
        self.activate(i);
        let answer = rfd::MessageDialog::new()
            .set_title("jot")
            .set_description(format!("Save changes to {}?", self.docs[i].file_name()))
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        match answer {
            rfd::MessageDialogResult::Yes => self.save(),
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    // ---- input ------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |mods: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(mods, key));
        let ctrl = Modifiers::CTRL;
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        let ctrl_alt = Modifiers::CTRL | Modifiers::ALT;

        // Chords with more modifiers first so they are not swallowed by simpler ones.
        if pressed(ctrl_shift, Key::S) {
            self.save_as();
        }
        if pressed(ctrl_shift, Key::F) {
            self.focus = !self.focus;
        }
        if pressed(ctrl_shift, Key::M) {
            let now = self.doc().is_markdown();
            self.doc_mut().markdown_override = Some(!now);
        }
        if pressed(ctrl_shift, Key::L) {
            self.cfg.line_numbers = !self.cfg.line_numbers;
            self.touch_config();
        }
        if pressed(ctrl_shift, Key::B) {
            self.cfg.tab_bar = self.cfg.tab_bar.next();
            self.touch_config();
            self.notify(format!("Tab bar: {}", self.cfg.tab_bar.label()));
        }
        if pressed(ctrl_shift, Key::Tab) {
            self.step_tab(false);
        }
        if pressed(ctrl_alt, Key::ArrowUp) {
            self.cfg.opacity = (self.cfg.opacity + 0.05).min(1.0);
            self.touch_config();
            self.notify(format!("Opacity {}%", (self.cfg.opacity * 100.0).round()));
        }
        if pressed(ctrl_alt, Key::ArrowDown) {
            self.cfg.opacity = (self.cfg.opacity - 0.05).max(MIN_OPACITY);
            self.touch_config();
            self.notify(format!("Opacity {}%", (self.cfg.opacity * 100.0).round()));
        }
        if pressed(Modifiers::SHIFT, Key::F3) {
            self.step_match(false);
        }
        if pressed(Modifiers::NONE, Key::F3) {
            self.step_match(true);
        }

        if pressed(ctrl, Key::Tab) {
            self.step_tab(true);
        }
        if pressed(ctrl, Key::T) || pressed(ctrl, Key::N) {
            self.new_tab();
        }
        if pressed(ctrl, Key::W) {
            self.close_tab(self.active);
        }
        if pressed(ctrl, Key::S) {
            self.save();
        }
        if pressed(ctrl, Key::O) {
            self.open_dialog();
        }
        if pressed(ctrl, Key::F) {
            self.find.open = true;
            self.find.focus_input = true;
        }
        if pressed(Modifiers::ALT, Key::Z) {
            self.cfg.word_wrap = !self.cfg.word_wrap;
            self.touch_config();
            self.notify(if self.cfg.word_wrap { "Word wrap on" } else { "Word wrap off" });
            // Windows also delivers the letter as text; keep it out of the document.
            drop_text(ctx, "z");
        }
        for (n, key) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9]
            .into_iter()
            .enumerate()
        {
            if pressed(Modifiers::ALT, key) {
                if n < self.docs.len() {
                    self.activate(n);
                }
                drop_text(ctx, &(n + 1).to_string());
            }
        }
        if pressed(Modifiers::NONE, Key::F1) {
            self.help_open = !self.help_open;
        }
        if pressed(Modifiers::NONE, Key::F11) {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.fullscreen));
        }
        if pressed(Modifiers::NONE, Key::Escape) {
            if self.help_open {
                self.help_open = false;
            } else if self.find.open {
                self.find.open = false;
                self.focus_editor = true;
            } else if self.focus {
                self.focus = false;
            }
        }

        // Font size: Ctrl +, Ctrl -, Ctrl 0 and Ctrl + scroll wheel.
        let mut size = self.cfg.font_size;
        for mods in [ctrl, ctrl_shift] {
            if pressed(mods, Key::Equals) || pressed(mods, Key::Plus) {
                size += 1.0;
            }
        }
        if pressed(ctrl, Key::Minus) {
            size -= 1.0;
        }
        if pressed(ctrl, Key::Num0) {
            size = Config::default().font_size;
        }
        let zoom = ctx.input(|i| i.zoom_delta());
        if (zoom - 1.0).abs() > f32::EPSILON {
            size *= zoom;
        }
        let size = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        if (size - self.cfg.font_size).abs() > 0.01 {
            self.cfg.font_size = size;
            self.touch_config();
            self.notify(format!("Font size {}", size.round()));
        }
    }

    fn handle_window_events(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        for path in dropped {
            self.open_path(path);
        }
        if !self.force_close && ctx.input(|i| i.viewport().close_requested()) && self.docs.iter().any(Doc::dirty) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            let all_ok = (0..self.docs.len()).all(|i| self.confirm_discard(i));
            if all_ok {
                self.force_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    // ---- config sync --------------------------------------------------------

    fn sync_config(&mut self, ctx: &egui::Context) {
        if let Some(since) = self.cfg_dirty_since {
            if since.elapsed() >= CFG_SAVE_DELAY {
                self.cfg.save();
                self.cfg_mtime = Config::modified();
                self.cfg_dirty_since = None;
            } else {
                ctx.request_repaint_after(CFG_SAVE_DELAY);
            }
        }
        if self.cfg_polled.elapsed() >= CFG_POLL {
            self.cfg_polled = Instant::now();
            let mtime = Config::modified();
            if self.cfg_dirty_since.is_none() && mtime != self.cfg_mtime {
                self.cfg_mtime = mtime;
                let new = Config::load();
                if new.font != self.cfg.font {
                    fonts::install(ctx, &new.font);
                }
                self.cfg = new;
                for doc in &mut self.docs {
                    doc.galley_cache = GalleyCache::default();
                }
            }
        }
        ctx.request_repaint_after(CFG_POLL);
    }

    // ---- find ---------------------------------------------------------------

    fn refresh_matches(&mut self) {
        let key = hash_of(&(self.active, self.doc().text_hash, &self.find.query));
        if key != self.find.matches_key {
            self.find.matches_key = key;
            self.find.matches = text_util::find_all(&self.doc().text, &self.find.query);
            self.find.current = self.find.current.min(self.find.matches.len().saturating_sub(1));
        }
    }

    fn step_match(&mut self, forward: bool) {
        self.refresh_matches();
        let n = self.find.matches.len();
        if n == 0 {
            return;
        }
        self.find.current = if forward { (self.find.current + 1) % n } else { (self.find.current + n - 1) % n };
        if let Some(m) = self.find.matches.get(self.find.current).cloned() {
            let doc = self.doc_mut();
            doc.pending_scroll = Some(m.start);
            doc.pending_select = Some((m.start, m.end));
        }
    }

    // ---- drawing ------------------------------------------------------------

    fn bg(&self, boost: f32) -> Color32 {
        let a = ((self.cfg.opacity + boost).clamp(0.0, 1.0) * 255.0) as u8;
        Color32::from_rgba_unmultiplied(BG[0], BG[1], BG[2], a)
    }

    /// The title bar doubles as the tab strip, like WezTerm's integrated tab bar.
    fn title_bar(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let rect = ui.max_rect();
        let drag = ui.interact(rect, ui.id().with("drag"), egui::Sense::click_and_drag());
        if drag.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!max));
        }

        let font = FontId::new(13.0, FontFamily::Monospace);
        if self.show_tabs() {
            self.tab_strip(ui, rect, &font);
        } else {
            let doc = self.doc();
            let title = format!("{}{}", if doc.dirty() { "\u{25CF} " } else { "" }, doc.file_name());
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, title, font, MUTED);
        }

        let button_w = 40.0;
        let labels = ["\u{2013}", "\u{25A1}", "\u{00D7}"];
        for (i, label) in labels.iter().enumerate() {
            let x = rect.right() - button_w * (labels.len() - i) as f32;
            let r = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(button_w, TITLE_BAR_HEIGHT));
            let resp = ui.interact(r, ui.id().with(("btn", i)), egui::Sense::click());
            if resp.hovered() {
                let hover = if i == 2 { Color32::from_rgb(0xac, 0x42, 0x42) } else { RAISED };
                ui.painter().rect_filled(r, 0.0, hover);
            }
            ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, FontId::new(14.0, FontFamily::Monospace), FG);
            if resp.clicked() {
                match i {
                    0 => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                    1 => {
                        let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(ViewportCommand::Maximized(!max));
                    }
                    _ => ctx.send_viewport_cmd(ViewportCommand::Close),
                }
            }
        }
    }

    fn tab_strip(&mut self, ui: &mut Ui, rect: egui::Rect, font: &FontId) {
        let mut action: Option<TabAction> = None;
        let max_x = rect.right() - 3.0 * 40.0 - 36.0;
        let mut x = rect.left() + 8.0;

        for i in 0..self.docs.len() {
            let doc = &self.docs[i];
            let active = i == self.active;
            let label = format!("{}{}", if doc.dirty() { "\u{25CF} " } else { "" }, doc.file_name());
            let text_w = ui.painter().layout_no_wrap(label.clone(), font.clone(), FG).size().x;
            let w = (text_w + 44.0).clamp(90.0, 220.0).min((max_x - x).max(60.0));
            let r = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 4.0), egui::vec2(w, TITLE_BAR_HEIGHT - 4.0));
            let id = ui.id().with(("tab", doc.id));

            let resp = ui.interact(r, id, egui::Sense::click());
            let close_r = egui::Rect::from_center_size(egui::pos2(r.right() - 15.0, r.center().y), egui::vec2(18.0, 18.0));
            let close = ui.interact(close_r, id.with("close"), egui::Sense::click());

            let hovered = resp.hovered() || close.hovered();
            let fill = if active {
                RAISED
            } else if hovered {
                Color32::from_rgb(0x20, 0x20, 0x20)
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(r, egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, fill);
            if active {
                let top = egui::Rect::from_min_size(r.left_top(), egui::vec2(r.width(), 2.0));
                ui.painter().rect_filled(top, 0.0, BLUE);
            }

            let clip = egui::Rect::from_min_max(r.left_top(), egui::pos2(r.right() - 26.0, r.bottom()));
            ui.painter().with_clip_rect(clip).text(
                egui::pos2(r.left() + 10.0, r.center().y + 1.0),
                Align2::LEFT_CENTER,
                label,
                font.clone(),
                if active { FG } else { MUTED },
            );

            if active || hovered {
                if close.hovered() {
                    ui.painter().rect_filled(close_r, 3.0, Color32::from_rgb(0x38, 0x38, 0x38));
                }
                ui.painter().text(
                    close_r.center(),
                    Align2::CENTER_CENTER,
                    "\u{00D7}",
                    FontId::new(14.0, FontFamily::Monospace),
                    if close.hovered() { RED } else { MUTED },
                );
            }

            if close.clicked() || resp.middle_clicked() {
                action = Some(TabAction::Close(i));
            } else if resp.clicked() {
                action = Some(TabAction::Activate(i));
            }
            x += w + 2.0;
            if x >= max_x {
                break;
            }
        }

        // "+" button.
        let plus = egui::Rect::from_min_size(egui::pos2(x + 2.0, rect.top() + 4.0), egui::vec2(28.0, TITLE_BAR_HEIGHT - 4.0));
        let resp = ui.interact(plus, ui.id().with("new-tab"), egui::Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(plus, 4.0, RAISED);
        }
        ui.painter().text(plus.center(), Align2::CENTER_CENTER, "+", FontId::new(16.0, FontFamily::Monospace), MUTED);
        if resp.clicked() {
            action = Some(TabAction::New);
        }

        match action {
            Some(TabAction::Activate(i)) => self.activate(i),
            Some(TabAction::Close(i)) => self.close_tab(i),
            Some(TabAction::New) => self.new_tab(),
            None => {}
        }
    }

    fn status_bar(&mut self, ui: &mut Ui, cursor_char: usize) {
        let rect = ui.max_rect();
        let font = FontId::new(12.0, FontFamily::Monospace);

        let left = match &self.status {
            Some((msg, at)) if at.elapsed() < Duration::from_secs(4) => msg.clone(),
            _ => self.doc().path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        };
        ui.painter().text(rect.left_center() + egui::vec2(12.0, 0.0), Align2::LEFT_CENTER, left, font.clone(), FAINT);

        let (line, col) = self.doc_mut().line_col(cursor_char);
        let words = self.doc().stats.1;
        let kind = if !self.doc().is_markdown() {
            "txt"
        } else if self.doc().text.len() > LARGE_FILE_BYTES {
            "md (plain, large file)"
        } else {
            "md"
        };
        let wrap = if self.cfg.word_wrap { "wrap" } else { "nowrap" };
        let right = format!("Ln {line}, Col {col}   {words} words   {kind}   {wrap}   F1 keys");
        ui.painter().text(rect.right_center() - egui::vec2(12.0, 0.0), Align2::RIGHT_CENTER, right, font, FAINT);
    }

    fn find_bar(&mut self, ui: &mut Ui) {
        self.refresh_matches();
        ui.horizontal_centered(|ui| {
            ui.add_space(12.0);
            ui.label(egui::RichText::new("find").color(MUTED).monospace());
            let resp = ui.add(
                TextEdit::singleline(&mut self.find.query)
                    .id(self.find_id)
                    .frame(Frame::NONE)
                    .desired_width(240.0)
                    .text_color(FG)
                    .font(FontId::new(14.0, FontFamily::Monospace)),
            );
            if self.find.focus_input {
                resp.request_focus();
                self.find.focus_input = false;
            }
            let n = self.find.matches.len();
            let info = if self.find.query.is_empty() {
                String::new()
            } else if n == 0 {
                "no matches".to_owned()
            } else {
                format!("{}/{}", self.find.current + 1, n)
            };
            let color = if n == 0 && !self.find.query.is_empty() { RED } else { MUTED };
            ui.label(egui::RichText::new(info).color(color).monospace());

            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let backwards = ui.input(|i| i.modifiers.shift);
                // The same Enter must not reach the editor and replace the selected match.
                ui.input_mut(|i| {
                    i.consume_key(Modifiers::NONE, Key::Enter);
                    i.consume_key(Modifiers::SHIFT, Key::Enter);
                });
                self.step_match(!backwards);
                self.focus_editor = true;
            }
        });
    }

    fn help_overlay(&mut self, ctx: &egui::Context) {
        let area = egui::Area::new(Id::new("jot-help"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                Frame::NONE
                    .fill(Color32::from_rgba_unmultiplied(0x20, 0x20, 0x20, 0xf5))
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgb(0x38, 0x38, 0x38)))
                    .corner_radius(8.0)
                    .inner_margin(Margin::same(22))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("keys").color(BLUE_BRIGHT).monospace().size(16.0));
                        ui.add_space(8.0);
                        egui::Grid::new("jot-keys").num_columns(2).spacing([28.0, 5.0]).show(ui, |ui| {
                            for (keys, what) in KEYS {
                                ui.label(egui::RichText::new(*keys).color(YELLOW).monospace().size(13.0));
                                ui.label(egui::RichText::new(*what).color(FG).monospace().size(13.0));
                                ui.end_row();
                            }
                        });
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("F1 or Esc to close").color(FAINT).monospace().size(12.0));
                    });
            });
        // A click outside the panel closes it.
        let clicked = ctx.input(|i| i.pointer.any_pressed());
        let inside = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| area.response.rect.contains(p));
        if clicked && !inside {
            self.help_open = false;
        }
    }

    fn editor(&mut self, ui: &mut Ui, ctx: &egui::Context) -> usize {
        // Work on the active document as a local so the closures below can borrow it freely.
        let mut doc = std::mem::take(&mut self.docs[self.active]);
        let editor_id = doc.editor_id();
        doc.refresh_stats();

        let size = self.cfg.font_size;
        let font = FontId::new(size, FontFamily::Monospace);
        let (char_w, row_h) = ctx.fonts_mut(|f| {
            let g = f.layout_no_wrap("0".to_owned(), font.clone(), FG);
            (g.size().x, g.size().y)
        });

        let markdown = doc.is_markdown() && doc.text.len() <= LARGE_FILE_BYTES;
        let show_numbers = self.cfg.line_numbers && !self.focus;
        let digits = doc.stats.2.to_string().len().max(2);
        let gutter = if show_numbers { (char_w * digits as f32 + 20.0).ceil() } else { 0.0 };
        let wrap = self.cfg.word_wrap;

        // Focus mode: a centered column and a dimmed background outside the current paragraph.
        let previous = TextEditState::load(ctx, editor_id).and_then(|s| s.cursor.char_range());
        let focus_range = self.focus.then(|| {
            let idx = previous.map_or(0, |r| r.primary.index.0);
            text_util::paragraph_range(&doc.text, text_util::char_to_byte(&doc.text, idx))
        });

        let avail = ui.available_rect_before_wrap();
        let column = if self.focus {
            let w = (self.cfg.focus_column * char_w + 2.0 * 24.0).min(avail.width());
            egui::Rect::from_center_size(avail.center(), egui::vec2(w, avail.height()))
        } else {
            avail
        };

        if let Some((start, end)) = doc.pending_select.take() {
            let mut state = TextEditState::load(ctx, editor_id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(start), CCursor::new(end))));
            state.store(ctx, editor_id);
            self.focus_editor = true;
        }
        if self.focus_editor {
            ctx.memory_mut(|m| m.request_focus(editor_id));
            self.focus_editor = false;
        }

        let mut cursor_char = 0;
        let focus_mode = self.focus;
        ui.scope_builder(UiBuilder::new().max_rect(column), |ui| {
            let scroll = if wrap { ScrollArea::vertical() } else { ScrollArea::both() };
            scroll.auto_shrink([false, false]).id_salt(("jot-scroll", doc.id)).show(ui, |ui| {
                let rows = ((ui.available_height() / row_h).floor() as usize).max(1);
                ui.add_space(if focus_mode { 32.0 } else { 6.0 });

                let text_hash = doc.text_hash;
                let ppp = ctx.pixels_per_point();
                let focus = focus_range.clone();
                let cache = &mut doc.galley_cache;
                let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| -> Arc<egui::Galley> {
                    let key = hash_of(&(
                        text_hash,
                        size.to_bits(),
                        markdown,
                        wrap_width.to_bits(),
                        wrap,
                        ppp.to_bits(),
                        focus.as_ref().map(|r| (r.start, r.end)),
                    ));
                    if let (Some(g), true) = (&cache.galley, cache.key == key) {
                        return g.clone();
                    }
                    let mut job =
                        highlight::build(buf.as_str(), &Options { markdown, font_size: size, focus: focus.as_ref() });
                    job.wrap.max_width = if wrap { wrap_width } else { f32::INFINITY };
                    let galley = ui.fonts_mut(|f| f.layout_job(job));
                    cache.key = key;
                    cache.galley = Some(galley.clone());
                    galley
                };

                let margin = Margin { left: (gutter as i8).saturating_add(8), right: 8, top: 0, bottom: 0 };
                let out = TextEdit::multiline(&mut doc.text)
                    .id(editor_id)
                    .font(font.clone())
                    .frame(Frame::NONE.inner_margin(margin))
                    .desired_width(ui.available_width())
                    .desired_rows(rows)
                    .lock_focus(true)
                    .layouter(&mut layouter)
                    .show(ui);

                if out.response.response.changed() {
                    doc.text_hash = hash_of(&doc.text);
                }
                cursor_char = out.cursor_range.map_or(0, |r| r.primary.index.0);

                if let Some(idx) = doc.pending_scroll.take() {
                    let r = out.galley.pos_from_cursor(CCursor::new(idx));
                    ui.scroll_to_rect(r.translate(out.galley_pos.to_vec2()), Some(Align::Center));
                }

                if show_numbers {
                    let current_line = doc.line_col(cursor_char).0;
                    let painter = ui.painter_at(ui.clip_rect());
                    let clip = ui.clip_rect();
                    let x = out.galley_pos.x - 10.0;
                    let mut line = 1;
                    let mut line_start = true;
                    for row in &out.galley.rows {
                        let top = out.galley_pos.y + row.pos.y;
                        let visible = top + row.size.y >= clip.top() && top <= clip.bottom();
                        if line_start && visible {
                            let bottom = top + row.size.y;
                            let color = if line == current_line { FG } else { FAINT };
                            painter.text(
                                egui::pos2(x, bottom - row_h * 0.5),
                                Align2::RIGHT_CENTER,
                                line.to_string(),
                                font.clone(),
                                color,
                            );
                        }
                        line_start = row.ends_with_newline;
                        if row.ends_with_newline {
                            line += 1;
                        }
                    }
                }
            });
        });

        let slot = self.active;
        self.docs[slot] = doc;
        cursor_char
    }
}

/// Removes a `Text` event for `s` produced by an Alt shortcut so it does not reach the document.
fn drop_text(ctx: &egui::Context, s: &str) {
    ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(t) if t.eq_ignore_ascii_case(s))));
}

fn configure_visuals(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = RAISED;
    v.extreme_bg_color = Color32::TRANSPARENT;
    v.override_text_color = Some(FG);
    v.selection.bg_fill = Color32::from_rgb(0x38, 0x38, 0x38);
    v.selection.stroke = egui::Stroke::new(1.0, BLUE_BRIGHT);
    v.text_cursor.stroke = egui::Stroke::new(2.0, FG);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    ctx.set_visuals(v);
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    ctx.global_style_mut(|s| {
        s.spacing.scroll = egui::style::ScrollStyle::thin();
        s.visuals.striped = false;
    });
}

impl eframe::App for Jot {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        self.sync_config(&ctx);
        self.shortcuts(&ctx);
        self.handle_window_events(&ctx);

        let title = {
            let d = self.doc();
            format!("{}{} - jot", if d.dirty() { "* " } else { "" }, d.file_name())
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }

        // Chrome is hidden in focus mode, except the title bar, which appears when the pointer nears the top.
        let near_top = ctx.input(|i| i.pointer.hover_pos().is_some_and(|p| p.y < TITLE_BAR_HEIGHT * 1.5));
        if !self.focus {
            egui::Panel::top("title_bar")
                .exact_size(TITLE_BAR_HEIGHT)
                .frame(Frame::NONE.fill(self.bg(0.0)))
                .show(ui, |ui| self.title_bar(ui, &ctx));
            egui::Panel::bottom("status_bar")
                .exact_size(STATUS_BAR_HEIGHT)
                .frame(Frame::NONE.fill(self.bg(0.0)))
                .show(ui, |ui| {
                    let cursor = TextEditState::load(&ctx, self.doc().editor_id())
                        .and_then(|s| s.cursor.char_range())
                        .map_or(0, |r| r.primary.index.0);
                    self.status_bar(ui, cursor);
                });
        }
        if self.find.open {
            egui::Panel::bottom("find_bar")
                .exact_size(30.0)
                .frame(Frame::NONE.fill(self.bg(0.05)))
                .show(ui, |ui| self.find_bar(ui));
        }

        egui::CentralPanel::default().frame(Frame::NONE.fill(self.bg(0.0))).show(ui, |ui| {
            self.editor(ui, &ctx);
        });

        if self.focus && near_top {
            egui::Area::new(Id::new("focus-title"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(0.0, 0.0))
                .show(&ctx, |ui| {
                    let width = ctx.content_rect().width();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, TITLE_BAR_HEIGHT), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, self.bg(0.0));
                    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| self.title_bar(ui, &ctx));
                });
        }

        if self.help_open {
            self.help_overlay(&ctx);
        }
    }
}
