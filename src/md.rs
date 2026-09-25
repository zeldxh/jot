//! A small, fast, line-oriented Markdown tokenizer for live syntax styling.
//!
//! It never changes the text: it only classifies byte ranges. The returned spans always tile the
//! whole input in order, and a span never crosses a line boundary.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    /// Syntax characters such as `#`, `**`, backticks, `>` and link brackets.
    Marker,
    Heading,
    Bold,
    Italic,
    Strike,
    Code,
    CodeBlock,
    Quote,
    Link,
    LinkUrl,
    ListMarker,
    TaskOpen,
    TaskDone,
    Rule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: Kind,
    /// Heading level (1 to 6) of the line this span is on, or 0.
    pub level: u8,
}

struct Builder {
    spans: Vec<Span>,
    cursor: usize,
    line_start: usize,
}

impl Builder {
    fn push(&mut self, range: Range<usize>, kind: Kind, level: u8) {
        if range.is_empty() {
            return;
        }
        if range.start > self.cursor {
            let gap = self.cursor..range.start;
            self.push_raw(gap, Kind::Text, 0);
        }
        self.push_raw(range, kind, level);
    }

    fn push_raw(&mut self, range: Range<usize>, kind: Kind, level: u8) {
        self.cursor = range.end;
        if let Some(last) = self.spans.last_mut() {
            if last.kind == kind
                && last.level == level
                && last.range.end == range.start
                && last.range.start >= self.line_start
            {
                last.range.end = range.end;
                return;
            }
        }
        self.spans.push(Span { range, kind, level });
    }
}

pub fn tokenize(text: &str) -> Vec<Span> {
    let mut b = Builder { spans: Vec::new(), cursor: 0, line_start: 0 };
    let mut fence: Option<(u8, usize)> = None;
    let mut offset = 0;

    for raw in text.split_inclusive('\n') {
        b.line_start = offset;
        let content_len = raw.trim_end_matches(['\n', '\r']).len();
        let content = &raw[..content_len];
        line(content, offset, &mut fence, &mut b);
        // Plain text left over at the end of the line, then the line break itself.
        if b.cursor < offset + content_len {
            let rest = b.cursor..offset + content_len;
            b.push_raw(rest, Kind::Text, 0);
        }
        b.push(offset + content_len..offset + raw.len(), Kind::Text, 0);
        offset += raw.len();
    }
    b.spans
}

fn line(s: &str, base: usize, fence: &mut Option<(u8, usize)>, b: &mut Builder) {
    if s.is_empty() {
        return;
    }
    let trimmed = s.trim_start();
    let indent = s.len() - trimmed.len();

    // Fenced code blocks. Inside one, only a matching fence line closes it.
    if let Some((ch, len)) = *fence {
        let run = trimmed.bytes().take_while(|&c| c == ch).count();
        if indent <= 3 && run >= len && trimmed[run..].trim().is_empty() {
            *fence = None;
            b.push(base..base + s.len(), Kind::Marker, 0);
        } else {
            b.push(base..base + s.len(), Kind::CodeBlock, 0);
        }
        return;
    }
    if indent <= 3 {
        for ch in [b'`', b'~'] {
            let run = trimmed.bytes().take_while(|&c| c == ch).count();
            // A backtick fence's info string cannot contain backticks: ```js code``` is inline code.
            let opens = run >= 3 && !(ch == b'`' && trimmed[run..].contains('`'));
            if opens {
                *fence = Some((ch, run));
                b.push(base..base + s.len(), Kind::Marker, 0);
                return;
            }
        }
    }

    // ATX headings.
    let hashes = s.bytes().take_while(|&c| c == b'#').count();
    if (1..=6).contains(&hashes) {
        let rest = &s[hashes..];
        if rest.is_empty() || rest.starts_with(' ') {
            let level = hashes as u8;
            let marker_end = hashes + usize::from(rest.starts_with(' '));
            b.push(base..base + marker_end, Kind::Marker, level);
            b.push(base + marker_end..base + s.len(), Kind::Heading, level);
            return;
        }
    }

    // Horizontal rules.
    if is_rule(trimmed) {
        b.push(base..base + s.len(), Kind::Rule, 0);
        return;
    }

    // Block quotes.
    if let Some(after) = trimmed.strip_prefix('>') {
        let marker_end = indent + 1 + usize::from(after.starts_with(' '));
        b.push(base + indent..base + marker_end, Kind::Marker, 0);
        b.push(base + marker_end..base + s.len(), Kind::Quote, 0);
        return;
    }

    // List items (with optional task box).
    if let Some(marker_len) = list_marker_len(trimmed) {
        let m_start = indent;
        let m_end = indent + marker_len;
        b.push(base + m_start..base + m_end, Kind::ListMarker, 0);
        let mut body = m_end;
        let after = &s[m_end..];
        // `after` starts with the space that follows the marker.
        let task = after.get(1..).unwrap_or("");
        for (tag, kind) in [("[ ]", Kind::TaskOpen), ("[x]", Kind::TaskDone), ("[X]", Kind::TaskDone)] {
            if task.starts_with(tag) && (task.len() == 3 || task[3..].starts_with(' ')) {
                b.push(base + m_end + 1..base + m_end + 4, kind, 0);
                body = m_end + 4;
                break;
            }
        }
        inline(&s[body..], base + body, b);
        return;
    }

    inline(s, base, b);
}

fn is_rule(t: &str) -> bool {
    let mut chars = t.chars().filter(|c| !c.is_whitespace());
    let Some(first) = chars.next() else { return false };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    let mut count = 1;
    for c in chars {
        if c != first {
            return false;
        }
        count += 1;
    }
    count >= 3
}

/// Length of a list marker (`-`, `*`, `+`, `1.` or `1)`) that is followed by a space.
fn list_marker_len(t: &str) -> Option<usize> {
    let bytes = t.as_bytes();
    let n = match bytes.first()? {
        b'-' | b'*' | b'+' => 1,
        b'0'..=b'9' => {
            let digits = bytes.iter().take_while(|c| c.is_ascii_digit()).count();
            match bytes.get(digits) {
                Some(b'.') | Some(b')') => digits + 1,
                _ => return None,
            }
        }
        _ => return None,
    };
    (bytes.get(n) == Some(&b' ')).then_some(n)
}

fn inline(s: &str, base: usize, b: &mut Builder) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\\' => {
                // Escaped character: leave both as plain text.
                i += 1;
                if i < bytes.len() {
                    i += s[i..].chars().next().map_or(1, char::len_utf8);
                }
            }
            b'`' => {
                // A code span opens with a run of N backticks and closes with the next run of exactly N.
                let n = bytes[i..].iter().take_while(|&&c| c == b'`').count();
                let mut from = i + n;
                let mut close = None;
                while let Some(rel) = s[from..].find('`') {
                    let k = from + rel;
                    let run = bytes[k..].iter().take_while(|&&c| c == b'`').count();
                    if run == n {
                        close = Some(k);
                        break;
                    }
                    from = k + run;
                }
                match close.filter(|&k| k > i + n) {
                    Some(k) => {
                        b.push(base + i..base + i + n, Kind::Marker, 0);
                        b.push(base + i + n..base + k, Kind::Code, 0);
                        b.push(base + k..base + k + n, Kind::Marker, 0);
                        i = k + n;
                    }
                    None => i += n,
                }
            }
            b'*' | b'_' | b'~' => {
                let double = bytes.get(i + 1) == Some(&c);
                if double {
                    if let Some(end) = emphasis(s, i, 2, c, base, b) {
                        i = end;
                        continue;
                    }
                    i += 2;
                } else if c != b'~' {
                    if let Some(end) = emphasis(s, i, 1, c, base, b) {
                        i = end;
                        continue;
                    }
                    i += 1;
                } else {
                    i += 1;
                }
            }
            b'[' => {
                if let Some(end) = link(s, i, base, b) {
                    i = end;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
}

/// Parses `**bold**`, `__bold__`, `~~strike~~`, `*italic*` and `_italic_` starting at `i`.
/// On success pushes the spans and returns the byte index after the closing delimiter.
fn emphasis(s: &str, i: usize, n: usize, c: u8, base: usize, b: &mut Builder) -> Option<usize> {
    let open_end = i + n;
    // `_` must not start or end inside a word (keeps snake_case intact).
    if c == b'_' {
        let prev = s[..i].chars().next_back();
        if prev.is_some_and(char::is_alphanumeric) {
            return None;
        }
    }
    let delim = &s[i..open_end];
    let mut from = open_end;
    let close = loop {
        let rel = s[from..].find(delim)?;
        let k = from + rel;
        // For single delimiters skip runs like `**` inside.
        if n == 1 && (s.as_bytes().get(k + 1) == Some(&c) || (k > open_end && s.as_bytes()[k - 1] == c)) {
            from = k + 1;
            continue;
        }
        break k;
    };
    let inner = &s[open_end..close];
    if inner.is_empty() || inner.starts_with(' ') || inner.ends_with(' ') {
        return None;
    }
    if c == b'_' {
        let next = s[close + n..].chars().next();
        if next.is_some_and(char::is_alphanumeric) {
            return None;
        }
    }
    let kind = match (c, n) {
        (b'~', _) => Kind::Strike,
        (_, 2) => Kind::Bold,
        _ => Kind::Italic,
    };
    b.push(base + i..base + open_end, Kind::Marker, 0);
    b.push(base + open_end..base + close, kind, 0);
    b.push(base + close..base + close + n, Kind::Marker, 0);
    Some(close + n)
}

/// Parses `[text](url)` starting at `i`.
fn link(s: &str, i: usize, base: usize, b: &mut Builder) -> Option<usize> {
    let mid = i + 1 + s[i + 1..].find("](")?;
    if mid == i + 1 {
        return None;
    }
    let url_start = mid + 2;
    let close = url_start + s[url_start..].find(')')?;
    b.push(base + i..base + i + 1, Kind::Marker, 0);
    b.push(base + i + 1..base + mid, Kind::Link, 0);
    b.push(base + mid..base + url_start, Kind::Marker, 0);
    b.push(base + url_start..base + close, Kind::LinkUrl, 0);
    b.push(base + close..base + close + 1, Kind::Marker, 0);
    Some(close + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (kind, text) pairs for easy assertions.
    fn view(text: &str) -> Vec<(Kind, &str)> {
        tokenize(text).into_iter().map(|s| (s.kind, &text[s.range])).collect()
    }

    #[test]
    fn heading_marker_and_text() {
        let spans = tokenize("# Title\n");
        assert_eq!(spans[0], Span { range: 0..2, kind: Kind::Marker, level: 1 });
        assert_eq!(spans[1], Span { range: 2..7, kind: Kind::Heading, level: 1 });
        assert_eq!(view("### Deep"), vec![(Kind::Marker, "### "), (Kind::Heading, "Deep")]);
    }

    #[test]
    fn hash_without_space_is_not_a_heading() {
        assert_eq!(view("#tag"), vec![(Kind::Text, "#tag")]);
        assert_eq!(view("####### seven"), vec![(Kind::Text, "####### seven")]);
    }

    #[test]
    fn inline_styles() {
        assert_eq!(
            view("a **b** c"),
            vec![(Kind::Text, "a "), (Kind::Marker, "**"), (Kind::Bold, "b"), (Kind::Marker, "**"), (Kind::Text, " c")]
        );
        assert_eq!(view("*hi*"), vec![(Kind::Marker, "*"), (Kind::Italic, "hi"), (Kind::Marker, "*")]);
        assert_eq!(view("~~x~~"), vec![(Kind::Marker, "~~"), (Kind::Strike, "x"), (Kind::Marker, "~~")]);
        assert_eq!(view("`code`"), vec![(Kind::Marker, "`"), (Kind::Code, "code"), (Kind::Marker, "`")]);
    }

    #[test]
    fn unclosed_delimiters_are_plain() {
        assert_eq!(view("**open"), vec![(Kind::Text, "**open")]);
        assert_eq!(view("a * b * c"), vec![(Kind::Text, "a * b * c")]);
    }

    #[test]
    fn snake_case_is_not_italic() {
        assert_eq!(view("my_var_name"), vec![(Kind::Text, "my_var_name")]);
        assert_eq!(view("_it_"), vec![(Kind::Marker, "_"), (Kind::Italic, "it"), (Kind::Marker, "_")]);
    }

    #[test]
    fn links() {
        assert_eq!(
            view("[a](http://x)"),
            vec![
                (Kind::Marker, "["),
                (Kind::Link, "a"),
                (Kind::Marker, "]("),
                (Kind::LinkUrl, "http://x"),
                (Kind::Marker, ")")
            ]
        );
        assert_eq!(view("[a] (b)"), vec![(Kind::Text, "[a] (b)")]);
    }

    #[test]
    fn fenced_code() {
        let v = view("```rs\nlet x = **1**;\n```\nafter");
        assert_eq!(v[0], (Kind::Marker, "```rs"));
        assert!(v.contains(&(Kind::CodeBlock, "let x = **1**;")));
        assert_eq!(*v.last().unwrap(), (Kind::Text, "after"));
    }

    #[test]
    fn triple_backticks_on_one_line_are_inline_code() {
        assert_eq!(
            view("```js console.log(\"carro\")```"),
            vec![(Kind::Marker, "```"), (Kind::Code, "js console.log(\"carro\")"), (Kind::Marker, "```")]
        );
        // It must not open a code block that swallows the following lines.
        let v = view("```a b```
next **b**");
        assert!(v.contains(&(Kind::Bold, "b")));
        assert!(!v.iter().any(|(k, _)| *k == Kind::CodeBlock));
        assert_eq!(
            view("x ``a`b`` y"),
            vec![(Kind::Text, "x "), (Kind::Marker, "``"), (Kind::Code, "a`b"), (Kind::Marker, "``"), (Kind::Text, " y")]
        );
    }

    #[test]
    fn fences_close_only_on_a_matching_fence() {
        let v = view("```js
code
```
after");
        assert_eq!(v[0], (Kind::Marker, "```js"));
        assert!(v.contains(&(Kind::CodeBlock, "code")));
        assert!(v.contains(&(Kind::Marker, "```")));
        assert_eq!(*v.last().unwrap(), (Kind::Text, "after"));

        // A shorter fence inside a longer one is just code; text after the closing fence is styled again.
        let v = view("````
```
x
````
**b**");
        assert!(v.contains(&(Kind::CodeBlock, "```")));
        assert!(v.contains(&(Kind::Bold, "b")));

        // A fence line with trailing text does not close the block.
        let v = view("```
``` not closing
still code");
        assert!(v.contains(&(Kind::CodeBlock, "``` not closing")));
        assert!(v.contains(&(Kind::CodeBlock, "still code")));
    }

    #[test]
    fn lists_and_tasks() {
        assert_eq!(view("- item"), vec![(Kind::ListMarker, "-"), (Kind::Text, " item")]);
        assert_eq!(view("12. n"), vec![(Kind::ListMarker, "12."), (Kind::Text, " n")]);
        let v = view("- [x] done **b**");
        assert_eq!(v[0], (Kind::ListMarker, "-"));
        assert_eq!(v[2], (Kind::TaskDone, "[x]"));
        assert!(v.contains(&(Kind::Bold, "b")));
        assert_eq!(view("- [ ] todo")[2], (Kind::TaskOpen, "[ ]"));
        assert_eq!(view("-no space"), vec![(Kind::Text, "-no space")]);
    }

    #[test]
    fn quotes_and_rules() {
        assert_eq!(view("> hi"), vec![(Kind::Marker, "> "), (Kind::Quote, "hi")]);
        assert_eq!(view("---"), vec![(Kind::Rule, "---")]);
        assert_eq!(view("* * *"), vec![(Kind::Rule, "* * *")]);
        assert_eq!(view("--"), vec![(Kind::Text, "--")]);
    }

    #[test]
    fn spans_tile_the_text_without_crossing_lines() {
        let text = "# H\r\nplain **b** and `c`\n\n> q\n- [ ] t\n```\ncode\n```\nüñí **ç**\n[l](u)\nend";
        let spans = tokenize(text);
        let mut cursor = 0;
        for s in &spans {
            assert_eq!(s.range.start, cursor, "gap before {s:?}");
            assert!(s.range.end > s.range.start);
            let piece = &text[s.range.clone()];
            let inner = piece.trim_end_matches(['\n', '\r']);
            assert!(!inner.contains('\n'), "span crosses a line: {piece:?}");
            cursor = s.range.end;
        }
        assert_eq!(cursor, text.len());
    }

    #[test]
    fn empty_and_unicode_do_not_panic() {
        assert!(tokenize("").is_empty());
        for t in ["*é*", "`ñ`", "[é](ü)", "**日本語**", "_é_", "\\*x*", "# é"] {
            let spans = tokenize(t);
            assert_eq!(spans.last().unwrap().range.end, t.len());
        }
    }
}
