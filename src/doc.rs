//! One open document (one tab): its text, file, undo/scroll identity and cached layout.

use crate::text_util;
use eframe::egui::{self, Id};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

pub fn hash_of<T: Hash + ?Sized>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Caches the last laid-out galley so unchanged frames do no text work.
#[derive(Default)]
pub struct GalleyCache {
    pub key: u64,
    pub galley: Option<Arc<egui::Galley>>,
}

pub struct Doc {
    /// Unique per tab, so each tab keeps its own cursor, undo history and scroll position.
    pub id: u64,
    pub text: String,
    pub path: Option<PathBuf>,
    pub crlf: bool,
    pub saved_hash: u64,
    pub text_hash: u64,
    pub markdown_override: Option<bool>,
    pub galley_cache: GalleyCache,
    /// (text hash, words, lines)
    pub stats: (u64, usize, usize),
    /// (text hash, cursor char index, (line, column))
    pub cursor_cache: (u64, usize, (usize, usize)),
    pub pending_scroll: Option<usize>,
    pub pending_select: Option<(usize, usize)>,
}

impl Default for Doc {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Doc {
    pub fn new(id: u64) -> Self {
        let h = hash_of("");
        Self {
            id,
            text: String::new(),
            path: None,
            crlf: false,
            saved_hash: h,
            text_hash: h,
            markdown_override: None,
            galley_cache: GalleyCache::default(),
            stats: (0, 0, 1),
            cursor_cache: (0, usize::MAX, (1, 1)),
            pending_scroll: None,
            pending_select: None,
        }
    }

    /// Builds a document from file bytes. The flag reports whether invalid UTF-8 was replaced.
    pub fn from_bytes(id: u64, path: Option<PathBuf>, bytes: &[u8]) -> (Self, bool) {
        let (text, crlf, lossy) = text_util::decode(bytes);
        let mut doc = Self::new(id);
        doc.text_hash = hash_of(&text);
        doc.saved_hash = doc.text_hash;
        doc.text = text;
        doc.path = path;
        doc.crlf = crlf;
        (doc, lossy)
    }

    pub fn dirty(&self) -> bool {
        self.text_hash != self.saved_hash
    }

    /// An untouched, unnamed document that can be replaced when opening a file.
    pub fn is_blank(&self) -> bool {
        self.path.is_none() && self.text.is_empty()
    }

    pub fn is_markdown(&self) -> bool {
        self.markdown_override.unwrap_or_else(|| match &self.path {
            None => true,
            Some(p) => matches!(
                p.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(),
                Some("md" | "markdown" | "mdown" | "mdx")
            ),
        })
    }

    pub fn file_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "untitled".to_owned(), |n| n.to_string_lossy().into_owned())
    }

    pub fn editor_id(&self) -> Id {
        Id::new(("jot-editor", self.id))
    }

    pub fn refresh_stats(&mut self) {
        if self.stats.0 != self.text_hash || self.stats.2 == 0 {
            self.stats = (self.text_hash, text_util::word_count(&self.text), text_util::line_count(&self.text));
        }
    }

    /// Line and column of a cursor, recomputed only when the text or cursor changed.
    pub fn line_col(&mut self, cursor_char: usize) -> (usize, usize) {
        if self.cursor_cache.0 != self.text_hash || self.cursor_cache.1 != cursor_char {
            self.cursor_cache = (self.text_hash, cursor_char, text_util::line_col(&self.text, cursor_char));
        }
        self.cursor_cache.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_documents_are_blank_and_clean() {
        let d = Doc::new(3);
        assert!(d.is_blank());
        assert!(!d.dirty());
        assert_eq!(d.file_name(), "untitled");
        assert!(d.is_markdown());
    }

    #[test]
    fn from_bytes_decodes_and_starts_clean() {
        let (d, lossy) = Doc::from_bytes(1, Some("notes.md".into()), b"a\r\nb");
        assert!(!lossy);
        assert_eq!(d.text, "a\nb");
        assert!(d.crlf);
        assert!(!d.dirty());
        assert_eq!(d.file_name(), "notes.md");
        assert!(!d.is_blank());
    }

    #[test]
    fn dirty_follows_the_text_hash() {
        let mut d = Doc::new(1);
        d.text.push('x');
        d.text_hash = hash_of(&d.text);
        assert!(d.dirty());
        d.saved_hash = d.text_hash;
        assert!(!d.dirty());
    }

    #[test]
    fn markdown_detection_by_extension_and_override() {
        let (mut d, _) = Doc::from_bytes(1, Some("a.rs".into()), b"");
        assert!(!d.is_markdown());
        d.markdown_override = Some(true);
        assert!(d.is_markdown());
        let (d, _) = Doc::from_bytes(2, Some("README.MD".into()), b"");
        assert!(d.is_markdown());
    }

    #[test]
    fn line_col_cache_tracks_text_changes() {
        let mut d = Doc::new(1);
        d.text = "ab\ncd".into();
        d.text_hash = hash_of(&d.text);
        assert_eq!(d.line_col(4), (2, 2));
        d.text = "abcd".into();
        d.text_hash = hash_of(&d.text);
        assert_eq!(d.line_col(4), (1, 5));
    }

    #[test]
    fn tabs_have_distinct_editor_ids() {
        assert_ne!(Doc::new(1).editor_id(), Doc::new(2).editor_id());
    }
}
