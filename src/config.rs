//! User configuration, stored at `~/.config/jot/config.toml`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MIN_FONT_SIZE: f32 = 8.0;
pub const MAX_FONT_SIZE: f32 = 72.0;
pub const MIN_OPACITY: f32 = 0.3;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Installed font family name, or a path to a `.ttf` / `.otf` file.
    pub font: String,
    pub font_size: f32,
    /// Window background opacity, 0.3 to 1.0.
    pub opacity: f32,
    pub word_wrap: bool,
    pub line_numbers: bool,
    /// Width of the text column in focus mode, in characters.
    pub focus_column: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: "IosevkaTerm Nerd Font Mono".to_owned(),
            font_size: 18.0,
            opacity: 0.92,
            word_wrap: true,
            line_numbers: true,
            focus_column: 72.0,
        }
    }
}

impl Config {
    /// Keeps every value inside a sane range.
    pub fn clamped(mut self) -> Self {
        self.font_size = self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        self.opacity = self.opacity.clamp(MIN_OPACITY, 1.0);
        self.focus_column = self.focus_column.clamp(30.0, 200.0);
        self
    }

    /// `JOT_CONFIG` overrides the location (used for testing without touching the real config).
    pub fn path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("JOT_CONFIG") {
            return Some(PathBuf::from(p));
        }
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
        Some(PathBuf::from(home).join(".config").join("jot").join("config.toml"))
    }

    /// Loads the config, falling back to defaults when it is missing or invalid.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| Self::parse(&s))
            .unwrap_or_default()
    }

    pub fn parse(text: &str) -> Self {
        toml::from_str::<Config>(text).unwrap_or_default().clamped()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = toml::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }

    pub fn modified() -> Option<std::time::SystemTime> {
        std::fs::metadata(Self::path()?).ok()?.modified().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty() {
        assert_eq!(Config::parse(""), Config::default());
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let c = Config::parse("font_size = 24.0\nword_wrap = false");
        assert_eq!(c.font_size, 24.0);
        assert!(!c.word_wrap);
        assert_eq!(c.opacity, Config::default().opacity);
    }

    #[test]
    fn invalid_toml_falls_back() {
        assert_eq!(Config::parse("this is = not [ valid"), Config::default());
    }

    #[test]
    fn values_are_clamped() {
        let c = Config::parse("font_size = 500.0\nopacity = 0.0");
        assert_eq!(c.font_size, MAX_FONT_SIZE);
        assert_eq!(c.opacity, MIN_OPACITY);
    }

    #[test]
    fn roundtrip() {
        let c = Config { font_size: 20.0, opacity: 0.5, ..Config::default() };
        let text = toml::to_string_pretty(&c).unwrap();
        assert_eq!(Config::parse(&text), c);
    }
}
