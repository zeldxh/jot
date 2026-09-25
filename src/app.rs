//! The jot window: title bar, editor with line numbers, find bar, status bar and focus mode.

use crate::config::{Config, MAX_FONT_SIZE, MIN_FONT_SIZE, MIN_OPACITY};
use crate::highlight::{self, Options, BG, BLUE_BRIGHT, FAINT, FG, MUTED, RAISED, RED};
use crate::{fonts, text_util};
use eframe::egui::{
    self, text::CCursor, text::CCursorRange, text_edit::TextEditState, Align, Align2, Color32, FontFamily,
    FontId, Frame, Id, Key, Margin, Modifiers, ScrollArea, TextEdit, Ui, UiBuilder,
    ViewportCommand,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

const TITLE_BAR_HEIGHT: f32 = 28.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const CFG_POLL: Duration = Duration::from_millis(1000);
const CFG_SAVE_DELAY: Duration = Duration::from_millis(600);

fn hash_of<T: Hash + ?Sized>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

#[derive(Default)]
struct Find {
    open: bool,
    query: String,
    focus_input: bool,
    matches: Vec<Range<usize>>,
    matches_key: u64,
    current: usize,
}

/// Caches the last laid-out galley so unchanged frames do no text work.
#[derive(Default)]
struct GalleyCache {
    key: u64,
    galley: Option<Arc<egui::Galley>>,
}

pub struct Jot {
    cfg: Config,
    cfg_mtime: Option<SystemTime>,
    cfg_polled: Instant,
    cfg_dirty_since: Option<Instant>,

    text: String,
    path: Option<PathBuf>,
    crlf: bool,
    saved_hash: u64,
    text_hash: u64,

    markdown_override: Option<bool>,
    focus: bool,
    fullscreen: bool,
    force_close: bool,
    first_frame: bool,

    find: Find,
    pending_scroll: Option<usize>,
    pending_select: Option<(usize, usize)>,
    galley_cache: GalleyCache,
    stats: (u64, usize),
    status: Option<(String, Instant)>,
    window_title: String,
    editor_id: Id,
    find_id: Id,
}

impl Jot {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        let cfg = Config::load();
        let ctx = &cc.egui_ctx;
        let font_found = fonts::install(ctx, &cfg.font);
        configure_visuals(ctx);

        let mut app = Self {
            cfg_mtime: Config::modified(),
            cfg_polled: Instant::now(),
            cfg_dirty_since: None,
            cfg,
            text: String::new(),
            path: None,
            crlf: false,
            saved_hash: hash_of(""),
            text_hash: hash_of(""),
            markdown_override: None,
            focus: false,
            fullscreen: false,
            force_close: false,
            first_frame: true,
            find: Find::default(),
            pending_scroll: None,
            pending_select: None,
            galley_cache: GalleyCache::default(),
            stats: (0, 0),
            status: None,
            window_title: String::new(),
            editor_id: Id::new("jot-editor"),
            find_id: Id::new("jot-find"),
        };
        if let Some(path) = file {
            app.open_path(path);
        }
        if !font_found {
            app.notify(format!("Font \"{}\" not found, using the default", app.cfg.font));
        }
        app
    }

    // ---- state helpers ----------------------------------------------------

    fn dirty(&self) -> bool {
        self.text_hash != self.saved_hash
    }

    fn is_markdown(&self) -> bool {
        self.markdown_override.unwrap_or_else(|| match &self.path {
            None => true,
            Some(p) => matches!(
                p.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(),
                Some("md" | "markdown" | "mdown" | "mdx")
            ),
        })
    }

    fn file_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "untitled".to_owned(), |n| n.to_string_lossy().into_owned())
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn touch_config(&mut self) {
        self.cfg_dirty_since = Some(Instant::now());
    }

    // ---- files ------------------------------------------------------------

    fn open_path(&mut self, path: PathBuf) {
        match std::fs::read(&path) {
            Ok(bytes) => {
                let (text, crlf, lossy) = text_util::decode(&bytes);
                self.set_document(text, Some(path), crlf);
                if lossy {
                    self.notify("Invalid UTF-8 found, some characters were replaced");
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // `jot new-file.md` starts an empty document that will be created on save.
                self.set_document(String::new(), Some(path), false);
            }
            Err(e) => self.notify(format!("Could not open file: {e}")),
        }
    }

    fn set_document(&mut self, text: String, path: Option<PathBuf>, crlf: bool) {
        self.text_hash = hash_of(&text);
        self.saved_hash = self.text_hash;
        self.text = text;
        self.path = path;
        self.crlf = crlf;
        self.markdown_override = None;
        self.find = Find::default();
    }

    fn save(&mut self) -> bool {
        let Some(path) = self.path.clone().or_else(|| self.pick_save_path()) else {
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
            .set_file_name(self.file_name())
            .add_filter("Markdown", &["md", "markdown"])
            .add_filter("Text", &["txt"])
            .add_filter("All files", &["*"])
            .save_file()
    }

    fn write_to(&mut self, path: PathBuf) -> bool {
        match std::fs::write(&path, text_util::encode(&self.text, self.crlf)) {
            Ok(()) => {
                self.saved_hash = self.text_hash;
                self.path = Some(path);
                self.notify("Saved");
                true
            }
            Err(e) => {
                self.notify(format!("Could not save: {e}"));
                false
            }
        }
    }

    fn open_dialog(&mut self) {
        if !self.confirm_discard() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            self.open_path(path);
        }
    }

    fn new_document(&mut self) {
        if self.confirm_discard() {
            self.set_document(String::new(), None, false);
        }
    }

    /// Asks what to do with unsaved changes. Returns `true` when it is fine to continue.
    fn confirm_discard(&mut self) -> bool {
        if !self.dirty() {
            return true;
        }
        let answer = rfd::MessageDialog::new()
            .set_title("jot")
            .set_description(format!("Save changes to {}?", self.file_name()))
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
            let now = self.is_markdown();
            self.markdown_override = Some(!now);
        }
        if pressed(ctrl_shift, Key::L) {
            self.cfg.line_numbers = !self.cfg.line_numbers;
            self.touch_config();
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

        if pressed(ctrl, Key::S) {
            self.save();
        }
        if pressed(ctrl, Key::O) {
            self.open_dialog();
        }
        if pressed(ctrl, Key::N) {
            self.new_document();
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
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(t) if t.eq_ignore_ascii_case("z"))));
        }
        if pressed(Modifiers::NONE, Key::F11) {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.fullscreen));
        }
        if pressed(Modifiers::NONE, Key::Escape) {
            if self.find.open {
                self.find.open = false;
                ctx.memory_mut(|m| m.request_focus(self.editor_id));
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
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            if self.confirm_discard() {
                self.open_path(path);
            }
        }
        if !self.force_close && ctx.input(|i| i.viewport().close_requested()) && self.dirty() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if self.confirm_discard() {
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
                self.galley_cache = GalleyCache::default();
            }
        }
        ctx.request_repaint_after(CFG_POLL);
    }

    // ---- find ---------------------------------------------------------------

    fn refresh_matches(&mut self) {
        let key = hash_of(&(self.text_hash, &self.find.query));
        if key != self.find.matches_key {
            self.find.matches_key = key;
            self.find.matches = text_util::find_all(&self.text, &self.find.query);
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
        self.jump_to_current_match();
    }

    fn jump_to_current_match(&mut self) {
        let Some(m) = self.find.matches.get(self.find.current).cloned() else { return };
        self.select_chars(m.start, m.end);
    }

    fn select_chars(&mut self, start: usize, end: usize) {
        self.pending_scroll = Some(start);
        self.pending_select = Some((start, end));
    }

    // ---- drawing ------------------------------------------------------------

    fn bg(&self, boost: f32) -> Color32 {
        let a = ((self.cfg.opacity + boost).clamp(0.0, 1.0) * 255.0) as u8;
        Color32::from_rgba_unmultiplied(BG[0], BG[1], BG[2], a)
    }

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

        let title = format!("{}{}", if self.dirty() { "\u{25CF} " } else { "" }, self.file_name());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            title,
            FontId::new(13.0, FontFamily::Monospace),
            MUTED,
        );

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

    fn status_bar(&mut self, ui: &mut Ui, cursor_char: usize) {
        let rect = ui.max_rect();
        let font = FontId::new(12.0, FontFamily::Monospace);
        let painter = ui.painter();

        let left = match &self.status {
            Some((msg, at)) if at.elapsed() < Duration::from_secs(4) => msg.clone(),
            _ => self.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        };
        painter.text(rect.left_center() + egui::vec2(12.0, 0.0), Align2::LEFT_CENTER, left, font.clone(), FAINT);

        let (line, col) = text_util::line_col(&self.text, cursor_char);
        let kind = if self.is_markdown() { "md" } else { "txt" };
        let wrap = if self.cfg.word_wrap { "wrap" } else { "nowrap" };
        let right = format!("Ln {line}, Col {col}   {} words   {kind}   {wrap}", self.stats.1);
        painter.text(rect.right_center() - egui::vec2(12.0, 0.0), Align2::RIGHT_CENTER, right, font, FAINT);
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
            ui.label(egui::RichText::new(info).color(if n == 0 && !self.find.query.is_empty() { RED } else { MUTED }).monospace());

            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let backwards = ui.input(|i| i.modifiers.shift);
                // The same Enter must not reach the editor and replace the selected match.
                ui.input_mut(|i| {
                    i.consume_key(Modifiers::NONE, Key::Enter);
                    i.consume_key(Modifiers::SHIFT, Key::Enter);
                });
                self.step_match(!backwards);
            }
        });
    }

    fn editor(&mut self, ui: &mut Ui, ctx: &egui::Context) -> usize {
        let size = self.cfg.font_size;
        let font = FontId::new(size, FontFamily::Monospace);
        let (char_w, row_h) = ctx.fonts_mut(|f| {
            let g = f.layout_no_wrap("0".to_owned(), font.clone(), FG);
            (g.size().x, g.size().y)
        });

        let markdown = self.is_markdown();
        let show_numbers = self.cfg.line_numbers && !self.focus;
        let digits = text_util::line_count(&self.text).to_string().len().max(2);
        let gutter = if show_numbers { (char_w * digits as f32 + 20.0).ceil() } else { 0.0 };
        let wrap = self.cfg.word_wrap;

        // Focus mode: a centered column and a dimmed background outside the current paragraph.
        let previous = TextEditState::load(ctx, self.editor_id).and_then(|s| s.cursor.char_range());
        let focus_range = self.focus.then(|| {
            let idx = previous.map_or(0, |r| r.primary.index.0);
            text_util::paragraph_range(&self.text, text_util::char_to_byte(&self.text, idx))
        });

        let avail = ui.available_rect_before_wrap();
        let column = if self.focus {
            let w = (self.cfg.focus_column * char_w + 2.0 * 24.0).min(avail.width());
            egui::Rect::from_center_size(avail.center(), egui::vec2(w, avail.height()))
        } else {
            avail
        };

        if let Some((start, end)) = self.pending_select.take() {
            let mut state = TextEditState::load(ctx, self.editor_id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(start), CCursor::new(end))));
            state.store(ctx, self.editor_id);
            ctx.memory_mut(|m| m.request_focus(self.editor_id));
        }

        let mut cursor_char = 0;
        ui.scope_builder(UiBuilder::new().max_rect(column), |ui| {
            let scroll = if wrap { ScrollArea::vertical() } else { ScrollArea::both() };
            scroll.auto_shrink([false, false]).id_salt("jot-scroll").show(ui, |ui| {
                let rows = ((ui.available_height() / row_h).floor() as usize).max(1);
                let top_pad = if self.focus { 32.0 } else { 6.0 };
                ui.add_space(top_pad);

                let text_hash = self.text_hash;
                let cache = &mut self.galley_cache;
                let ppp = ctx.pixels_per_point();
                let focus = focus_range.clone();
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
                    let mut job = highlight::build(buf.as_str(), &Options { markdown, font_size: size, focus: focus.as_ref() });
                    job.wrap.max_width = if wrap { wrap_width } else { f32::INFINITY };
                    let galley = ui.fonts_mut(|f| f.layout_job(job));
                    cache.key = key;
                    cache.galley = Some(galley.clone());
                    galley
                };

                let margin = Margin { left: (gutter as i8).saturating_add(8), right: 8, top: 0, bottom: 0 };
                let out = TextEdit::multiline(&mut self.text)
                    .id(self.editor_id)
                    .font(font.clone())
                    .frame(Frame::NONE.inner_margin(margin))
                    .desired_width(ui.available_width())
                    .desired_rows(rows)
                    .lock_focus(true)
                    .layouter(&mut layouter)
                    .show(ui);

                if out.response.response.changed() {
                    self.text_hash = hash_of(&self.text);
                }
                cursor_char = out.cursor_range.map_or(0, |r| r.primary.index.0);

                if let Some(idx) = self.pending_scroll.take() {
                    let r = out.galley.pos_from_cursor(CCursor::new(idx));
                    ui.scroll_to_rect(r.translate(out.galley_pos.to_vec2()), Some(Align::Center));
                }

                if show_numbers {
                    let current_line = text_util::line_col(&self.text, cursor_char).0;
                    let painter = ui.painter_at(ui.clip_rect());
                    let num_font = FontId::new(size, FontFamily::Monospace);
                    let x = out.galley_pos.x - 10.0;
                    let mut line = 1;
                    let mut line_start = true;
                    for row in &out.galley.rows {
                        if line_start {
                            let bottom = out.galley_pos.y + row.pos.y + row.size.y;
                            let color = if line == current_line { FG } else { FAINT };
                            painter.text(
                                egui::pos2(x, bottom - row_h * 0.5),
                                Align2::RIGHT_CENTER,
                                line.to_string(),
                                num_font.clone(),
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
        cursor_char
    }
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

        if self.first_frame {
            self.first_frame = false;
            ctx.memory_mut(|m| m.request_focus(self.editor_id));
        }
        self.sync_config(&ctx);
        self.shortcuts(&ctx);
        self.handle_window_events(&ctx);
        self.text_hash = hash_of(&self.text);
        if self.stats.0 != self.text_hash {
            self.stats = (self.text_hash, text_util::word_count(&self.text));
        }

        let title = format!("{}{} - jot", if self.dirty() { "* " } else { "" }, self.file_name());
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }

        // Chrome: hidden in focus mode, except the title bar which appears when the pointer nears the top.
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
                    let cursor = TextEditState::load(&ctx, self.editor_id)
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

    }
}
