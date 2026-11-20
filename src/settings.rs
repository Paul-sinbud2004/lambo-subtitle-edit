use crate::i18n::Language;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Built-in colour schemes. `System` follows the desktop light/dark setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
    System,
    Sepia,
    Nord,
    HighContrast,
}

impl ColorScheme {
    pub fn is_dark(self) -> bool {
        matches!(self, ColorScheme::Dark | ColorScheme::Nord)
    }

}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub language: Language,
    #[serde(default)]
    pub color_scheme: ColorScheme,
    pub global_font_size: f32,
    pub editor_font_size: f32,
    pub preview_font_size: f32,
    /// RGBA floats in 0.0..=1.0
    pub preview_color: [f32; 4],
    pub step_seconds: f64,
    pub font_path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::Zh,
            color_scheme: ColorScheme::Light,
            global_font_size: 15.0,
            editor_font_size: 18.0,
            preview_font_size: 30.0,
            preview_color: [1.0, 1.0, 1.0, 1.0],
            step_seconds: 1.0,
            font_path: detect_system_font().unwrap_or_default(),
        }
    }
}

impl Settings {
    fn config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "srt_editor", "srt_editor")
            .map(|d| d.config_dir().join("settings.json"))
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(partial) = serde_json::from_str::<Settings>(&text) {
                    // An intentionally empty font_path means "use system default";
                    // respect it instead of re-detecting on every launch.
                    return partial;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(text) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(&path, text);
            }
        }
    }
}

/// Ask fontconfig for a CJK-capable sans-serif font file.
pub fn detect_system_font() -> Option<String> {
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "sans:lang=zh-cn"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}
