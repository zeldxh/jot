//! Pure text helpers (kept free of UI code so they can be unit tested).

use std::ops::Range;

/// Decodes file bytes. Returns the text with `\n` line endings, whether the file used CRLF,
/// and whether invalid UTF-8 had to be replaced.
pub fn decode(bytes: &[u8]) -> (String, bool, bool) {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let (text, lossy) = match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_owned(), false),
        Err(_) => (String::from_utf8_lossy(bytes).into_owned(), true),
    };
    let crlf = text.contains("\r\n");
    let text = if crlf { text.replace("\r\n", "\n") } else { text };
    (text, crlf, lossy)
}

pub fn encode(text: &str, crlf: bool) -> Vec<u8> {
    if crlf { text.replace('\n', "\r\n").into_bytes() } else { text.as_bytes().to_vec() }
}

/// 1-based line and column of a character index.
pub fn line_col(text: &str, char_idx: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for c in text.chars().take(char_idx) {
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

pub fn line_count(text: &str) -> usize {
    text.bytes().filter(|&b| b == b'\n').count() + 1
}

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

pub fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices().nth(char_idx).map_or(text.len(), |(b, _)| b)
}

/// The blank-line-delimited paragraph around a byte position (or just the blank line itself).
pub fn paragraph_range(text: &str, pos: usize) -> Range<usize> {
    let pos = pos.min(text.len());
    let line_of = |p: usize| -> Range<usize> {
        let start = text[..p].rfind('\n').map_or(0, |i| i + 1);
        let end = text[p..].find('\n').map_or(text.len(), |i| p + i + 1);
        start..end
    };
    let is_blank = |r: &Range<usize>| text[r.clone()].trim().is_empty();

    let cur = line_of(pos.min(text.len().saturating_sub(1)).max(0));
    if text.is_empty() || is_blank(&cur) {
        return cur;
    }
    let mut start = cur.start;
    while start > 0 {
        let prev = line_of(start - 1);
        if is_blank(&prev) {
            break;
        }
        start = prev.start;
    }
    let mut end = cur.end;
    while end < text.len() {
        let next = line_of(end);
        if is_blank(&next) {
            break;
        }
        end = next.end;
    }
    start..end
}

/// Case-insensitive search. Returns matches as character-index ranges.
pub fn find_all(text: &str, needle: &str) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let hay: Vec<char> = text.chars().map(fold).collect();
    let pat: Vec<char> = needle.chars().map(fold).collect();
    if pat.len() > hay.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + pat.len() <= hay.len() {
        if hay[i..i + pat.len()] == pat[..] {
            out.push(i..i + pat.len());
            i += pat.len();
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_handles_bom_and_crlf() {
        let (t, crlf, lossy) = decode(b"\xEF\xBB\xBFa\r\nb");
        assert_eq!((t.as_str(), crlf, lossy), ("a\nb", true, false));
        let (t, crlf, _) = decode(b"a\nb");
        assert_eq!((t.as_str(), crlf), ("a\nb", false));
    }

    #[test]
    fn decode_flags_invalid_utf8() {
        let (t, _, lossy) = decode(&[b'a', 0xFF, b'b']);
        assert!(lossy);
        assert!(t.starts_with('a') && t.ends_with('b'));
    }

    #[test]
    fn encode_roundtrip() {
        let (t, crlf, _) = decode(b"x\r\ny\r\n");
        assert_eq!(encode(&t, crlf), b"x\r\ny\r\n");
        assert_eq!(encode("x\ny", false), b"x\ny");
    }

    #[test]
    fn line_col_counts_from_one() {
        assert_eq!(line_col("ab\ncd", 0), (1, 1));
        assert_eq!(line_col("ab\ncd", 4), (2, 2));
        assert_eq!(line_col("", 0), (1, 1));
        assert_eq!(line_count("a\nb\n"), 3);
    }

    #[test]
    fn char_byte_conversions_with_unicode() {
        let t = "aéb";
        assert_eq!(char_to_byte(t, 2), 3);
        assert_eq!(char_to_byte(t, 99), t.len());
    }

    #[test]
    fn paragraphs() {
        let t = "one\ntwo\n\nthree\nfour\n";
        assert_eq!(&t[paragraph_range(t, 1)], "one\ntwo\n");
        assert_eq!(&t[paragraph_range(t, 12)], "three\nfour\n");
        assert_eq!(&t[paragraph_range(t, 8)], "\n"); // blank line
        assert_eq!(paragraph_range("", 0), 0..0);
        assert_eq!(&t[paragraph_range(t, t.len())], "three\nfour\n");
    }

    #[test]
    fn find_is_case_insensitive_and_char_based() {
        assert_eq!(find_all("Foo foo FOO", "foo"), vec![0..3, 4..7, 8..11]);
        assert_eq!(find_all("éa É", "é"), vec![0..1, 3..4]);
        assert!(find_all("abc", "").is_empty());
        assert!(find_all("ab", "abc").is_empty());
    }

    #[test]
    fn words() {
        assert_eq!(word_count("  a  b\nc "), 3);
    }
}
