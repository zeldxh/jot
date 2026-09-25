//! Loads a user-chosen system font into egui.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Font family used for bold text (headings and `**bold**`).
pub fn bold_family() -> FontFamily {
    FontFamily::Name("jot-bold".into())
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(w) = std::env::var_os("WINDIR") {
        dirs.push(PathBuf::from(w).join("Fonts"));
    }
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
    }
    dirs
}

/// Lowercase and drop everything that is not a letter or digit.
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

#[derive(Clone, Copy, PartialEq)]
pub enum Weight {
    Regular,
    Bold,
}

/// Whether a font file stem such as `IosevkaTermNerdFontMono-Bold` is the wanted style of `family`.
fn matches_style(stem: &str, family: &str, weight: Weight) -> bool {
    let stem = norm(stem);
    let family = norm(family);
    let Some(rest) = stem.strip_prefix(&family) else { return false };
    let has = |w: &str| rest.contains(w);
    if has("italic") || has("oblique") {
        return false;
    }
    match weight {
        Weight::Bold => has("bold") && !has("extra") && !has("semi") && !has("ultra"),
        Weight::Regular => {
            let styled = ["bold", "light", "thin", "medium", "semi", "extra", "heavy", "black", "ultra"];
            rest.is_empty() || rest == "regular" || (has("regular") && !styled.iter().any(|w| has(w)))
        }
    }
}

/// Finds a font file for a family name (or accepts a direct file path).
pub fn find_font(family: &str, weight: Weight) -> Option<PathBuf> {
    let direct = Path::new(family);
    if weight == Weight::Regular && direct.is_file() {
        return Some(direct.to_path_buf());
    }
    for dir in font_dirs() {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if ext != "ttf" && ext != "otf" {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if matches_style(stem, family, weight) {
                return Some(path);
            }
        }
    }
    None
}

/// Installs the configured font. Returns `false` when it was not found and the built-in
/// monospace font is used instead.
pub fn install(ctx: &egui::Context, family: &str) -> bool {
    let mut defs = FontDefinitions::default();
    let regular = find_font(family, Weight::Regular).and_then(|p| std::fs::read(p).ok());
    let bold = find_font(family, Weight::Bold).and_then(|p| std::fs::read(p).ok());

    let found = regular.is_some();
    let mut bold_chain = vec![];

    if let Some(bytes) = regular {
        defs.font_data.insert("jot-regular".into(), Arc::new(FontData::from_owned(bytes)));
        for fam in [FontFamily::Monospace, FontFamily::Proportional] {
            defs.families.entry(fam).or_default().insert(0, "jot-regular".into());
        }
        bold_chain.push("jot-regular".to_owned());
    }
    if let Some(bytes) = bold {
        defs.font_data.insert("jot-bold".into(), Arc::new(FontData::from_owned(bytes)));
        bold_chain.insert(0, "jot-bold".to_owned());
    }
    // Fall back to the built-in monospace chain so a missing bold still renders.
    if let Some(mono) = defs.families.get(&FontFamily::Monospace).cloned() {
        for name in mono {
            if !bold_chain.contains(&name) {
                bold_chain.push(name);
            }
        }
    }
    defs.families.insert(bold_family(), bold_chain);
    ctx.set_fonts(defs);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_matching() {
        let fam = "IosevkaTerm Nerd Font Mono";
        assert!(matches_style("IosevkaTermNerdFontMono-Regular", fam, Weight::Regular));
        assert!(matches_style("IosevkaTermNerdFontMono-Bold", fam, Weight::Bold));
        assert!(!matches_style("IosevkaTermNerdFontMono-BoldItalic", fam, Weight::Bold));
        assert!(!matches_style("IosevkaTermNerdFontMono-ExtraBold", fam, Weight::Bold));
        assert!(!matches_style("IosevkaTermNerdFontMono-Light", fam, Weight::Regular));
        assert!(!matches_style("Consolas", fam, Weight::Regular));
    }

    #[test]
    fn plain_family_file_is_regular() {
        assert!(matches_style("consola", "Consolas", Weight::Regular) == false);
        assert!(matches_style("Consolas", "Consolas", Weight::Regular));
    }
}
