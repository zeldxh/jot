//! Turns text into a styled egui `LayoutJob` (Alacritty palette, live Markdown styling).

use crate::fonts::bold_family;
use crate::md::{self, Kind, Span};
use eframe::egui::{
    text::{LayoutJob, TextFormat},
    Color32, FontFamily, FontId, Stroke,
};
use std::ops::Range;

// Alacritty palette
pub const BG: [u8; 3] = [0x18, 0x18, 0x18];
pub const RAISED: Color32 = Color32::from_rgb(0x28, 0x28, 0x28);
pub const FG: Color32 = Color32::from_rgb(0xd8, 0xd8, 0xd8);
pub const FG_BRIGHT: Color32 = Color32::from_rgb(0xf8, 0xf8, 0xf8);
pub const MUTED: Color32 = Color32::from_rgb(0x7f, 0x7f, 0x7f);
pub const FAINT: Color32 = Color32::from_rgb(0x55, 0x55, 0x55);
pub const RED: Color32 = Color32::from_rgb(0xd0, 0x60, 0x60);
pub const GREEN: Color32 = Color32::from_rgb(0x90, 0xa9, 0x59);
pub const YELLOW: Color32 = Color32::from_rgb(0xf4, 0xbf, 0x75);
pub const BLUE: Color32 = Color32::from_rgb(0x6a, 0x9f, 0xb5);
pub const BLUE_BRIGHT: Color32 = Color32::from_rgb(0x82, 0xb8, 0xc8);
pub const CYAN: Color32 = Color32::from_rgb(0x75, 0xb5, 0xaa);
const CODE_BG: Color32 = Color32::from_rgb(0x28, 0x28, 0x28);

/// Font size multiplier per heading level (index = level).
const HEADING_SCALE: [f32; 7] = [1.0, 1.9, 1.6, 1.35, 1.2, 1.1, 1.05];

pub struct Options<'a> {
    pub markdown: bool,
    pub font_size: f32,
    /// When set, everything outside this byte range is dimmed (focus mode).
    pub focus: Option<&'a Range<usize>>,
}

/// Mixes a color towards the background, used to dim text in focus mode.
fn dim(c: Color32) -> Color32 {
    let mix = |v: u8, bg: u8| (f32::from(v) * 0.32 + f32::from(bg) * 0.68) as u8;
    Color32::from_rgb(mix(c.r(), BG[0]), mix(c.g(), BG[1]), mix(c.b(), BG[2]))
}

fn format(span: &Span, size: f32) -> TextFormat {
    let mono = |s: f32| FontId::new(s, FontFamily::Monospace);
    let bold = |s: f32| FontId::new(s, bold_family());
    let level = usize::from(span.level.min(6));
    let heading_size = size * HEADING_SCALE[level];

    let mut f = TextFormat::simple(mono(size), FG);
    match span.kind {
        Kind::Text => {}
        Kind::Marker => {
            f.color = FAINT;
            if level > 0 {
                f.font_id = bold(heading_size);
            }
        }
        Kind::Heading => {
            f.font_id = bold(heading_size);
            f.color = if level <= 2 { FG_BRIGHT } else { BLUE_BRIGHT };
        }
        Kind::Bold => {
            f.font_id = bold(size);
            f.color = FG_BRIGHT;
        }
        Kind::Italic => {
            f.italics = true;
            f.color = Color32::from_rgb(0xc2, 0x8c, 0xb8);
        }
        Kind::Strike => {
            f.color = MUTED;
            f.strikethrough = Stroke::new(1.0, MUTED);
        }
        Kind::Code => {
            f.color = CYAN;
            f.background = CODE_BG;
        }
        Kind::CodeBlock => {
            f.color = GREEN;
            f.background = CODE_BG;
        }
        Kind::Quote => {
            f.color = MUTED;
            f.italics = true;
        }
        Kind::Link => {
            f.color = BLUE;
            f.underline = Stroke::new(1.0, BLUE);
        }
        Kind::LinkUrl => f.color = MUTED,
        Kind::ListMarker => f.color = YELLOW,
        Kind::TaskOpen => f.color = YELLOW,
        Kind::TaskDone => f.color = GREEN,
        Kind::Rule => f.color = FAINT,
    }
    f
}

pub fn build(text: &str, opts: &Options) -> LayoutJob {
    let mut job = LayoutJob::default();
    let plain = |job: &mut LayoutJob, s: &str, dimmed: bool| {
        if s.is_empty() && !job.sections.is_empty() {
            return;
        }
        let mut f = TextFormat::simple(FontId::new(opts.font_size, FontFamily::Monospace), FG);
        if dimmed {
            f.color = dim(f.color);
        }
        job.append(s, 0.0, f);
    };

    if !opts.markdown {
        // Focus dimming still works for plain text: split at the focused range.
        match opts.focus {
            Some(r) if r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end) => {
                plain(&mut job, &text[..r.start], true);
                plain(&mut job, &text[r.clone()], false);
                plain(&mut job, &text[r.end..], true);
            }
            _ => plain(&mut job, text, false),
        }
        return job;
    }

    let spans = md::tokenize(text);
    if spans.is_empty() {
        plain(&mut job, "", false);
        return job;
    }
    for span in &spans {
        let mut f = format(span, opts.font_size);
        if let Some(r) = opts.focus {
            let inside = span.range.start < r.end && span.range.end > r.start;
            if !inside {
                f.color = dim(f.color);
                if f.background != Color32::TRANSPARENT {
                    f.background = Color32::TRANSPARENT;
                }
                f.underline = Stroke::NONE;
                f.strikethrough = Stroke::NONE;
            }
        }
        job.append(&text[span.range.clone()], 0.0, f);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(markdown: bool) -> Options<'static> {
        Options { markdown, font_size: 16.0, focus: None }
    }

    #[test]
    fn job_text_matches_input() {
        for md in [true, false] {
            for t in ["", "plain", "# T\n**b** `c`\n\n- [ ] x\n", "é\r\nü"] {
                assert_eq!(build(t, &opts(md)).text, t);
            }
        }
    }

    #[test]
    fn headings_are_larger_than_body() {
        let job = build("# Big\nsmall", &opts(true));
        let size = |i: usize| job.sections[i].format.font_id.size;
        assert!(size(1) > size(job.sections.len() - 1));
    }

    #[test]
    fn focus_dims_outside_the_range() {
        let text = "one\n\ntwo\n";
        let range = 0..4;
        let job = build(text, &Options { markdown: true, font_size: 16.0, focus: Some(&range) });
        let first = &job.sections[0].format.color;
        let last = &job.sections[job.sections.len() - 1].format.color;
        assert_eq!(*first, FG);
        assert_ne!(*last, FG);
    }

    #[test]
    fn plain_focus_splits_text() {
        let range = 2..4;
        let job = build("abcdef", &Options { markdown: false, font_size: 16.0, focus: Some(&range) });
        assert_eq!(job.text, "abcdef");
        assert_eq!(job.sections.len(), 3);
    }
}
