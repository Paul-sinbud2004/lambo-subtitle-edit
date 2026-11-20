use crate::i18n::{self, Language, Strings};
use crate::settings::{ColorScheme, Settings};
use crate::srt::{self, Subtitle, SubtitleFile};
use crate::video::VideoPlayer;
use eframe::egui;
use egui_phosphor::regular as ico;
use egui::{
    Align2, Color32, Context, CornerRadius, DragValue, FontData, FontDefinitions, FontFamily,
    FontId, Id, Layout, Rect, RichText, TextureHandle, TextureOptions, Vec2, pos2,
};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const VIDEO_FILTER: &[(&str, &[&str])] = &[(
    "Video",
    &["mp4", "mkv", "avi", "mov", "webm", "m4v", "ts", "flv", "wmv", "mpeg", "mpg"],
)];

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlayMode {
    Normal,
    SubtitleOnly,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AudioSource {
    Muted,
    Target,
    Reference,
}

#[derive(Clone)]
enum PendingAction {
    LoadTarget(PathBuf),
    LoadReference(PathBuf),
    LoadSubtitleFile(PathBuf),
    LoadSubtitle,
    NewSubtitle,
}

/// Colours used by the IDE-style subtitle editor, derived from the active scheme.
#[derive(Clone, Copy)]
struct Palette {
    time_start: Color32,
    time_end: Color32,
    start_btn: Color32,
    end_btn: Color32,
    sub_text: Color32,
    line_no: Color32,
    selected: Color32,
    faint: Color32,
    text: Color32,
}

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn palette_for(scheme: ColorScheme) -> Palette {
    match scheme {
        ColorScheme::Dark => Palette {
            time_start: rgb(121, 192, 255),
            time_end: rgb(210, 168, 255),
            start_btn: rgb(21, 61, 102),
            end_btn: rgb(61, 42, 95),
            sub_text: rgb(230, 237, 243),
            line_no: rgb(110, 118, 129),
            selected: rgb(31, 75, 122),
            faint: rgb(33, 38, 45),
            text: rgb(230, 237, 243),
        },
        ColorScheme::Sepia => Palette {
            time_start: rgb(30, 110, 140),
            time_end: rgb(138, 74, 107),
            start_btn: rgb(214, 232, 236),
            end_btn: rgb(240, 221, 232),
            sub_text: rgb(74, 59, 40),
            line_no: rgb(168, 152, 120),
            selected: rgb(232, 217, 168),
            faint: rgb(237, 227, 204),
            text: rgb(74, 59, 40),
        },
        ColorScheme::Nord => Palette {
            time_start: rgb(136, 192, 208),
            time_end: rgb(180, 142, 173),
            start_btn: rgb(59, 84, 104),
            end_btn: rgb(87, 67, 90),
            sub_text: rgb(236, 239, 244),
            line_no: rgb(123, 136, 161),
            selected: rgb(76, 106, 128),
            faint: rgb(67, 76, 94),
            text: rgb(236, 239, 244),
        },
        ColorScheme::HighContrast => Palette {
            time_start: rgb(0, 255, 255),
            time_end: rgb(255, 0, 255),
            start_btn: rgb(0, 64, 64),
            end_btn: rgb(64, 0, 64),
            sub_text: rgb(255, 255, 0),
            line_no: rgb(170, 170, 170),
            selected: rgb(0, 64, 128),
            faint: rgb(26, 26, 26),
            text: rgb(255, 255, 255),
        },
        // Light and System both use the light palette.
        ColorScheme::Light | ColorScheme::System => Palette {
            time_start: rgb(9, 105, 218),
            time_end: rgb(130, 80, 223),
            start_btn: rgb(221, 238, 255),
            end_btn: rgb(243, 232, 255),
            sub_text: rgb(31, 35, 40),
            line_no: rgb(140, 149, 159),
            selected: rgb(221, 238, 255),
            faint: rgb(240, 242, 245),
            text: rgb(31, 35, 40),
        },
    }
}

fn scheme_label(scheme: ColorScheme, s: &Strings) -> &'static str {
    match scheme {
        ColorScheme::Light => s.scheme_light,
        ColorScheme::Dark => s.scheme_dark,
        ColorScheme::System => s.scheme_system,
        ColorScheme::Sepia => s.scheme_sepia,
        ColorScheme::Nord => s.scheme_nord,
        ColorScheme::HighContrast => s.scheme_contrast,
    }
}

/// Icon-style compact button with a hover tooltip.
fn icon_button(ui: &mut egui::Ui, icon: &str, tip: &str, size: f32) -> bool {
    ui.add(
        egui::Button::new(RichText::new(icon).size(size))
            .min_size(Vec2::new(34.0, 30.0))
            .corner_radius(CornerRadius::same(6)),
    )
    .on_hover_text(tip)
    .clicked()
}

/// Same look as `icon_button`, but greyed out and non-interactive when disabled.
fn icon_button_enabled(
    ui: &mut egui::Ui,
    icon: &str,
    tip: &str,
    size: f32,
    enabled: bool,
) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(icon).size(size))
            .min_size(Vec2::new(34.0, 30.0))
            .corner_radius(CornerRadius::same(6)),
    )
    .on_hover_text(tip)
    .clicked()
}

pub struct SubtitleEditor {
    settings: Settings,
    settings_open: bool,
    about_open: bool,
    error_message: Option<String>,
    pending: Option<PendingAction>,
    status: String,
    applied_font: Option<String>,

    target: VideoPlayer,
    reference: VideoPlayer,
    has_reference: bool,

    subs: SubtitleFile,
    /// Multi-selection of subtitle rows (supports shift-range and direct toggling).
    selected_rows: BTreeSet<usize>,
    anchor_row: Option<usize>,
    /// Timestamp currently chosen for the toolbar fine-tune buttons: (row, is_start).
    active_time: Option<(usize, bool)>,
    /// Drag state for block selection over line numbers.
    block_dragging: bool,
    block_anchor: Option<usize>,
    clipboard: Vec<Subtitle>,
    time_buffers: HashMap<(usize, bool), String>,
    /// Simple find & replace state for the subtitle editor.
    find_open: bool,
    find_text: String,
    replace_text: String,
    active_find_row: Option<usize>,
    pending_scroll_row: Option<usize>,

    playing: bool,
    play_mode: PlayMode,
    loop_selected: bool,
    /// Current subtitle row in "subtitle only" / "loop selected" playback.
    playback_cursor: Option<usize>,
    audio_source: AudioSource,
    baseline: f64,
    last_logic_seek: Instant,
    last_ref_sync: Instant,
    preview_pause_at: Option<Instant>,
    ref_preview_pause_at: Option<Instant>,

    /// Visual settings snapshot; `apply_settings` only runs when it changes.
    applied_visual: Option<(ColorScheme, f32, String)>,
    /// Video textures are rebuilt only when a new frame actually arrives.
    target_tex: Option<TextureHandle>,
    ref_tex: Option<TextureHandle>,
    target_frame_ver: u64,
    ref_frame_ver: u64,
    /// Set while a pipeline is prerolling its first frame after a load.
    target_loading_since: Option<Instant>,
    reference_loading_since: Option<Instant>,
    /// Command-line videos are loaded on the first UI frame so the loading
    /// indicator can be painted while the pipelines preroll.
    initial_load: Option<(PathBuf, Option<PathBuf>)>,
}

impl SubtitleEditor {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();
        let target = VideoPlayer::new().expect("无法创建视频播放器，请检查 GStreamer 安装");
        let reference = VideoPlayer::new().expect("无法创建参考视频播放器");
        let mut app = Self {
            settings,
            settings_open: false,
            about_open: false,
            error_message: None,
            pending: None,
            status: String::new(),
            applied_font: None,
            target,
            reference,
            has_reference: false,
            subs: SubtitleFile::default(),
            selected_rows: BTreeSet::new(),
            anchor_row: None,
            active_time: None,
            block_dragging: false,
            block_anchor: None,
            clipboard: Vec::new(),
            time_buffers: HashMap::new(),
            find_open: false,
            find_text: String::new(),
            replace_text: String::new(),
            active_find_row: None,
            pending_scroll_row: None,
            playing: false,
            play_mode: PlayMode::Normal,
            loop_selected: false,
            playback_cursor: None,
            audio_source: AudioSource::Muted,
            baseline: 0.0,
            last_logic_seek: Instant::now(),
            last_ref_sync: Instant::now(),
            preview_pause_at: None,
            ref_preview_pause_at: None,
            applied_visual: None,
            target_tex: None,
            ref_tex: None,
            target_frame_ver: u64::MAX,
            ref_frame_ver: u64::MAX,
            target_loading_since: None,
            reference_loading_since: None,
            initial_load: None,
        };
        app.apply_settings(&cc.egui_ctx);
        app.applied_visual = Some(app.visual_signature());
        app
    }

    fn visual_signature(&self) -> (ColorScheme, f32, String) {
        (
            self.settings.color_scheme,
            self.settings.global_font_size,
            self.settings.font_path.clone(),
        )
    }

    pub fn open_initial(&mut self, path: PathBuf, reference: Option<PathBuf>) {
        self.initial_load = Some((path, reference));
    }

    fn run_initial_load(&mut self) {
        let Some((target, reference)) = self.initial_load.take() else { return };
        self.load_target_video(target);
        if let Some(path) = reference {
            self.load_reference_video(path);
        }
    }

    fn lang(&self) -> &'static Strings {
        i18n::strings(self.settings.language)
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }

    // ---------------------------------------------------------------- settings

    fn build_fonts(path: &str) -> FontDefinitions {
        let mut fonts = FontDefinitions::default();
        if !path.trim().is_empty() {
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert(
                    "AppFont".to_owned(),
                    Arc::new(FontData::from_owned(bytes)),
                );
                if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                    family.insert(0, "AppFont".to_owned());
                }
                if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
                    family.insert(0, "AppFont".to_owned());
                }
            }
        }
        // Phosphor vector icons as a fallback in the proportional font.
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        fonts
    }

    fn apply_settings(&mut self, ctx: &Context) {
        ctx.set_theme(match self.settings.color_scheme {
            ColorScheme::System => egui::ThemePreference::System,
            scheme if scheme.is_dark() => egui::ThemePreference::Dark,
            _ => egui::ThemePreference::Light,
        });

        let pal = palette_for(self.settings.color_scheme);
        let scale = (self.settings.global_font_size / 15.0).clamp(0.5, 3.0);
        let mut text_styles = egui::Style::default().text_styles;
        for font in text_styles.values_mut() {
            font.size *= scale;
        }
        ctx.all_styles_mut(|style| {
            style.text_styles = text_styles.clone();
            style.spacing.item_spacing = Vec2::new(10.0, 7.0);
            style.spacing.button_padding = Vec2::new(9.0, 5.0);
            style.spacing.interact_size = Vec2::new(40.0, 32.0);
            style.spacing.window_margin = 14.0.into();
            style.spacing.menu_margin = 10.0.into();
            style.spacing.slider_rail_height = 8.0;
            for widgets in [
                &mut style.visuals.widgets.noninteractive,
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
            ] {
                widgets.corner_radius = CornerRadius::same(6);
            }
            style.visuals.window_corner_radius = CornerRadius::same(10);
            if self.settings.color_scheme != ColorScheme::System {
                style.visuals.window_fill = pal.faint;
                style.visuals.panel_fill = pal.faint;
                style.visuals.faint_bg_color = pal.faint;
                style.visuals.extreme_bg_color = pal.faint;
                style.visuals.widgets.noninteractive.fg_stroke.color = pal.text;
                style.visuals.widgets.inactive.fg_stroke.color = pal.text;
                style.visuals.widgets.hovered.fg_stroke.color = pal.text;
                style.visuals.widgets.active.fg_stroke.color = pal.text;
                style.visuals.selection.bg_fill = pal.selected;
            }
        });

        let path = self.settings.font_path.clone();
        if self.applied_font.as_deref() != Some(path.as_str()) {
            self.applied_font = Some(path.clone());
            ctx.set_fonts(Self::build_fonts(&path));
        }
    }

    // ----------------------------------------------------------------- actions

    fn request_action(&mut self, action: PendingAction) {
        self.pending = Some(action);
        if !self.subs.dirty {
            self.execute_pending();
        }
    }

    fn execute_pending(&mut self) {
        let Some(action) = self.pending.take() else { return };
        match action {
            PendingAction::LoadTarget(path) => self.load_target_video(path),
            PendingAction::LoadReference(path) => self.load_reference_video(path),
            PendingAction::LoadSubtitle => self.pick_subtitle_and_load(),
            PendingAction::LoadSubtitleFile(path) => self.load_subtitle_file(path),
            PendingAction::NewSubtitle => {
                self.subs = SubtitleFile {
                    entries: vec![Subtitle::new(0.0, 3.0, String::new())],
                    path: None,
                    dirty: true,
                };
                self.clear_selection();
                self.time_buffers.clear();
                self.find_text.clear();
                self.active_find_row = None;
                self.pending_scroll_row = None;
                self.loop_selected = false;
                self.set_status(self.lang().no_subtitle_loaded);
            }
        }
    }

    fn load_target_video(&mut self, path: PathBuf) {
        match self.target.load(&path) {
            Ok(()) => {
                self.target_loading_since = Some(Instant::now());
                    self.pause();
                self.clear_selection();
                self.time_buffers.clear();
                self.prime_preview();
                self.set_status(format!("{}: {}", self.lang().status_video_loaded, path.display()));
                if let Some(sidecar) = srt::find_sidecar(&path) {
                    self.request_action(PendingAction::LoadSubtitleFile(sidecar));
                }
            }
            Err(e) => self.report_error(e),
        }
    }

    fn load_reference_video(&mut self, path: PathBuf) {
        match self.reference.load(&path) {
            Ok(()) => {
                self.reference_loading_since = Some(Instant::now());
                self.has_reference = true;
                if self.playing {
                    let t = self.target.position() + self.baseline;
                    let _ = self.reference.play();
                    self.reference.seek(t.max(0.0));
                }
                self.apply_audio_source();
                if !self.playing {
                    self.prime_preview();
                }
                self.set_status(format!("{}: {}", self.lang().status_video_loaded, path.display()));
            }
            Err(e) => self.report_error(e),
        }
    }

    fn unload_reference(&mut self) {
        self.reference.unload();
        self.has_reference = false;
        self.apply_audio_source();
    }

    fn pick_video() -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new();
        for (name, exts) in VIDEO_FILTER {
            dialog = dialog.add_filter(*name, exts);
        }
        dialog.pick_file()
    }

    fn pick_subtitle_and_load(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SubRip", &["srt"])
            .pick_file()
        {
            self.load_subtitle_file(path);
        }
    }

    fn load_subtitle_file(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.subs = SubtitleFile {
                    entries: srt::parse_srt(&text),
                    path: Some(path.clone()),
                    dirty: false,
                };
                self.clear_selection();
                self.time_buffers.clear();
                self.find_text.clear();
                self.active_find_row = None;
                self.pending_scroll_row = None;
                self.loop_selected = false;
                self.set_status(format!("{}: {}", self.lang().status_subtitle_loaded, path.display()));
            }
            Err(e) => self.report_error(format!("无法读取字幕文件: {e}")),
        }
    }

    fn save_subtitle(&mut self, save_as: bool) {
        if self.subs.entries.is_empty() {
            self.set_status(self.lang().no_subtitle_loaded);
            return;
        }
        let path = if save_as || self.subs.path.is_none() {
            rfd::FileDialog::new()
                .add_filter("SubRip", &["srt"])
                .set_file_name("subtitle.srt")
                .save_file()
        } else {
            self.subs.path.clone()
        };
        let Some(path) = path else { return };
        match std::fs::write(&path, srt::serialize_srt(&self.subs.entries)) {
            Ok(()) => {
                self.subs.path = Some(path.clone());
                self.subs.dirty = false;
                self.set_status(format!("{}: {}", self.lang().status_subtitle_saved, path.display()));
            }
            Err(e) => self.report_error(format!("无法保存字幕: {e}")),
        }
    }

    /// Briefly run the pipelines so the decoder emits the first frame, then
    /// pause again. This keeps the preview from being blank while stopped.
    fn prime_preview(&mut self) {
        let _ = self.target.play();
        if self.has_reference {
            let _ = self.reference.play();
        }
        self.preview_pause_at = Some(Instant::now() + Duration::from_millis(180));
    }

    fn report_error(&mut self, msg: String) {
        self.error_message = Some(msg);
        self.target_loading_since = None;
        self.reference_loading_since = None;
        self.pause();
    }

    // ------------------------------------------------------- selection helpers

    fn clear_selection(&mut self) {
        self.selected_rows.clear();
        self.anchor_row = None;
        self.active_time = None;
        self.block_dragging = false;
        self.block_anchor = None;
        self.playback_cursor = None;
    }

    fn select_only(&mut self, idx: usize) {
        self.selected_rows.clear();
        self.selected_rows.insert(idx);
        self.anchor_row = Some(idx);
    }

    fn select_toggle(&mut self, idx: usize) {
        if self.selected_rows.contains(&idx) {
            self.selected_rows.remove(&idx);
        } else {
            self.selected_rows.insert(idx);
        }
        self.anchor_row = Some(idx);
    }

    fn select_range(&mut self, idx: usize) {
        if let Some(anchor) = self.anchor_row {
            self.select_range_from(anchor, idx);
        } else {
            self.select_only(idx);
        }
    }

    /// Select every row between `anchor` and `idx` (block selection helper).
    fn select_range_from(&mut self, anchor: usize, idx: usize) {
        self.selected_rows.clear();
        for i in anchor.min(idx)..=anchor.max(idx) {
            self.selected_rows.insert(i);
        }
        self.anchor_row = Some(anchor);
    }

    fn select_all_rows(&mut self) {
        self.selected_rows = (0..self.subs.entries.len()).collect();
        self.anchor_row = None;
    }

    /// Apply a toolbar fine-tune step to the timestamp the user last picked.
    fn adjust_active_time(&mut self, delta: f64) {
        let Some((row, is_start)) = self.active_time else { return };
        let Some(entry) = self.subs.entries.get_mut(row) else {
            self.active_time = None;
            return;
        };
        let base = if is_start { entry.start } else { entry.end };
        let wanted = (base + delta).max(0.0);
        if is_start {
            entry.start = wanted.min(entry.end);
        } else {
            entry.end = wanted.max(entry.start);
        }
        let actual = if is_start { entry.start } else { entry.end };
        self.time_buffers.insert((row, is_start), srt::format_time(actual));
        self.subs.dirty = true;
    }

    // ------------------------------------------------------- find & replace

    /// First row at or after `from` (wrapping) whose text contains `needle`.
    fn find_match_from(&self, needle: &str, from: usize) -> Option<usize> {
        let n = self.subs.entries.len();
        if needle.is_empty() || n == 0 {
            return None;
        }
        for step in 0..n {
            let i = (from + step) % n;
            if self.subs.entries[i].text.contains(needle) {
                return Some(i);
            }
        }
        None
    }

    /// Jump to the next matching subtitle, selecting it and showing its frame.
    fn find_next(&mut self, ctx: &Context) {
        let needle = self.find_text.clone();
        if needle.is_empty() {
            return;
        }
        let start = self
            .active_find_row
            .map(|r| (r + 1) % self.subs.entries.len().max(1))
            .unwrap_or(0);
        if let Some(row) = self.find_match_from(&needle, start) {
            self.active_find_row = Some(row);
            self.select_only(row);
            self.pending_scroll_row = Some(row);
            if !self.playing {
                self.seek_both(self.subs.entries[row].start);
                self.show_seeked_frame(ctx);
            }
            self.set_status(format!("{}: {}", self.lang().find_next, row + 1));
        } else {
            self.set_status(self.lang().find_not_found);
        }
        ctx.request_repaint();
    }

    /// Replace the match at the current find row, then advance to the next one.
    fn replace_current(&mut self, ctx: &Context) {
        let needle = self.find_text.clone();
        if needle.is_empty() {
            return;
        }
        let Some(row) = self.active_find_row else {
            self.find_next(ctx);
            return;
        };
        let mut changed = false;
        if let Some(entry) = self.subs.entries.get_mut(row) {
            if entry.text.contains(&needle) {
                entry.text = entry.text.replacen(&needle, &self.replace_text, 1);
                changed = true;
            }
        }
        if changed {
            self.subs.dirty = true;
            self.find_next(ctx);
        }
    }

    /// Replace every occurrence in every subtitle. Returns the match count.
    fn replace_all(&mut self) -> usize {
        let needle = self.find_text.clone();
        if needle.is_empty() {
            return 0;
        }
        let mut count = 0;
        for entry in &mut self.subs.entries {
            let hits = entry.text.matches(&needle).count();
            if hits > 0 {
                entry.text = entry.text.replace(&needle, &self.replace_text);
                count += hits;
            }
        }
        if count > 0 {
            self.subs.dirty = true;
            self.active_find_row = None;
        }
        count
    }

    /// Every subtitle, in line-number order. Used by "subtitle segments only",
    /// which deliberately ignores the checkbox selection.
    fn all_indices(&self) -> Vec<usize> {
        (0..self.subs.entries.len()).collect()
    }

    /// Only checked rows, in line-number order. Used by "loop selected segments".
    fn selected_indices(&self) -> Vec<usize> {
        self.selected_rows
            .iter()
            .copied()
            .filter(|&i| i < self.subs.entries.len())
            .collect()
    }

    fn delete_selected(&mut self) {
        if self.selected_rows.is_empty() {
            return;
        }
        let rows: Vec<usize> = self.selected_rows.iter().copied().collect();
        for &i in rows.iter().rev() {
            if i < self.subs.entries.len() {
                self.subs.entries.remove(i);
            }
        }
        self.subs.dirty = true;
        self.clear_selection();
        self.time_buffers.clear();
    }

    fn copy_selected(&mut self) {
        self.clipboard = self
            .selected_rows
            .iter()
            .filter_map(|&i| self.subs.entries.get(i).cloned())
            .collect();
    }

    fn cut_selected(&mut self) {
        self.copy_selected();
        self.delete_selected();
    }

    fn paste_rows(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let at = self
            .selected_rows
            .iter()
            .max()
            .map(|&i| i + 1)
            .unwrap_or(self.subs.entries.len());
        let src = self.clipboard.clone();
        let base = if at > 0 {
            self.subs.entries[at - 1].end
        } else {
            0.0
        };
        let shift = base - src[0].start;
        let mut new_sel = BTreeSet::new();
        for (k, mut item) in src.into_iter().enumerate() {
            item.start = (item.start + shift).max(0.0);
            item.end = (item.end + shift).max(item.start);
            new_sel.insert(at + k);
            self.subs.entries.insert(at + k, item);
        }
        self.subs.dirty = true;
        self.selected_rows = new_sel;
        self.active_time = None;
        self.time_buffers.clear();
    }

    fn add_row(&mut self) {
        let (start, at) = match self.selected_rows.iter().max() {
            Some(&i) if i < self.subs.entries.len() => {
                (self.subs.entries[i].end + 0.2, i + 1)
            }
            _ => (
                self.subs
                    .entries
                    .last()
                    .map(|e| e.end + 0.2)
                    .unwrap_or(0.0),
                self.subs.entries.len(),
            ),
        };
        self.subs
            .entries
            .insert(at, Subtitle::new(start, start + 2.0, String::new()));
        self.subs.dirty = true;
        self.select_only(at);
        self.active_time = None;
        self.time_buffers.clear();
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let (ctrl, shift, a, x, c, v, del, back) = ctx.input(|i| {
            (
                i.modifiers.ctrl || i.modifiers.command,
                i.modifiers.shift,
                i.key_pressed(egui::Key::A),
                i.key_pressed(egui::Key::X),
                i.key_pressed(egui::Key::C),
                i.key_pressed(egui::Key::V),
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::Backspace),
            )
        });
        // Only handle row-level shortcuts when no text field has focus, so
        // TextEdit's own Ctrl+A/C/V still work normally.
        let focus_free = ctx.memory(|m| m.focused().is_none());
        if !focus_free {
            return;
        }
        if ctrl && a {
            if shift {
                self.clear_selection();
            } else {
                self.select_all_rows();
            }
        }
        if ctrl && x {
            self.cut_selected();
        }
        if ctrl && c {
            self.copy_selected();
        }
        if ctrl && v {
            self.paste_rows();
        }
        if del || back {
            self.delete_selected();
        }
    }

    // ---------------------------------------------------------------- playback

    fn apply_audio_source(&self) {
        match self.audio_source {
            AudioSource::Muted => {
                self.target.set_muted(true);
                self.reference.set_muted(true);
            }
            AudioSource::Target => {
                self.target.set_muted(false);
                self.reference.set_muted(true);
            }
            AudioSource::Reference => {
                self.target.set_muted(true);
                self.reference.set_muted(false);
            }
        }
    }

    fn seek_both(&mut self, t: f64) {
        if self.target.path.is_none() {
            return;
        }
        self.target.seek(t);
        if self.has_reference {
            self.reference.seek((t + self.baseline).max(0.0));
        }
        self.last_logic_seek = Instant::now();
        self.last_ref_sync = Instant::now();
    }

    /// Keep the reference preview in sync with the baseline while it is being
    /// adjusted. When paused we briefly run the pipeline so a freshly decoded
    /// frame at the new offset is displayed, then pause again.
    fn on_baseline_changed(&mut self, ctx: &Context) {
        if !self.has_reference {
            ctx.request_repaint();
            return;
        }
        let desired = (self.target.position() + self.baseline).max(0.0);
        if self.playing {
            self.reference.seek(desired);
            self.last_ref_sync = Instant::now();
        } else {
            let _ = self.reference.play();
            self.reference.seek(desired);
            self.ref_preview_pause_at = Some(Instant::now() + Duration::from_millis(180));
        }
        ctx.request_repaint();
    }

    /// After a transport seek while paused, briefly run both pipelines so the
    /// freshly decoded frame at the new position is shown in both previews.
    fn show_seeked_frame(&mut self, ctx: &Context) {
        ctx.request_repaint();
        if self.playing {
            return;
        }
        self.prime_preview();
    }

    fn next_playback_after(&self, t: f64) -> Option<usize> {
        // "Next subtitle" is a plain transport action: always use all subtitles.
        let rows = self.all_indices();
        let mut best: Option<(usize, f64)> = None;
        for i in rows {
            let s = &self.subs.entries[i];
            if s.start > t + 1e-6 && best.is_none_or(|(_, bt)| s.start < bt) {
                best = Some((i, s.start));
            }
        }
        best.map(|(i, _)| i)
    }

    fn play(&mut self) {
        if self.target.path.is_none() {
            return;
        }
        if self.target.duration > 0.0 && self.target.position() >= self.target.duration - 0.1 {
            self.seek_both(0.0);
        }

        self.playback_cursor = None;
        if self.loop_selected {
            // Loop uses only checked rows, restarting from the lowest line number.
            let rows = self.selected_indices();
            if let Some(&first) = rows.first() {
                self.playback_cursor = Some(first);
                self.seek_both(self.subs.entries[first].start);
            }
        } else if self.play_mode == PlayMode::SubtitleOnly {
            // Subtitle-only ignores the selection and plays every segment once,
            // always starting from the first subtitle in line-number order.
            let rows = self.all_indices();
            if let Some(&first) = rows.first() {
                self.playback_cursor = Some(first);
                self.seek_both(self.subs.entries[first].start);
            }
        }

        let _ = self.target.play();
        if self.has_reference {
            let _ = self.reference.play();
        }
        self.playing = true;
    }

    fn pause(&mut self) {
        self.target.pause();
        if self.has_reference {
            self.reference.pause();
        }
        self.playing = false;
    }

    fn stop(&mut self) {
        self.pause();
        self.playback_cursor = None;
        self.seek_both(0.0);
        // Show the frame at 0 in both previews even while stopped.
        self.prime_preview();
    }

    fn poll_video(&mut self) {
        self.target.poll_bus();
        self.reference.poll_bus();
        let mut errors = self.target.take_errors();
        errors.extend(self.reference.take_errors());
        let eos = self.target.take_eos() || self.reference.take_eos();
        if !errors.is_empty() {
            self.report_error(errors.join("\n"));
        } else if eos {
            self.pause();
        }
        self.target.refresh_duration();
        if self.has_reference {
            self.reference.refresh_duration();
        }
        // The first decoded frame means the loading indicator can stop.
        if self.target_loading_since.is_some() && self.target.frame().is_some() {
            self.target_loading_since = None;
        }
        if self.reference_loading_since.is_some() && self.reference.frame().is_some() {
            self.reference_loading_since = None;
        }
        if let Some(deadline) = self.preview_pause_at {
            if Instant::now() >= deadline {
                if !self.playing {
                    self.target.pause();
                    if self.has_reference && self.ref_preview_pause_at.is_none() {
                        self.reference.pause();
                    }
                }
                self.preview_pause_at = None;
            }
        }
        // After a baseline tweak while paused, let the reference decode one
        // fresh frame at the new position, then pause again.
        if let Some(deadline) = self.ref_preview_pause_at {
            if Instant::now() >= deadline {
                if !self.playing {
                    self.reference.pause();
                }
                self.ref_preview_pause_at = None;
            }
        }
    }

    fn tick_playback(&mut self) {
        if !self.playing || self.target.path.is_none() {
            return;
        }
        let mut t = self.target.position();

        // Two independent behaviours:
        // - loop selected: only checked rows, wrapping to the first checked row;
        // - subtitle-only: every row in order, stopping after the last one.
        if self.loop_selected || self.play_mode == PlayMode::SubtitleOnly {
            let rows = if self.loop_selected {
                self.selected_indices()
            } else {
                self.all_indices()
            };
            if rows.is_empty() {
                self.playback_cursor = None;
                if self.play_mode == PlayMode::SubtitleOnly && !self.loop_selected {
                    self.pause();
                }
            } else {
                if self.playback_cursor.is_none_or(|i| !rows.contains(&i)) {
                    let mut chosen = None;
                    for &i in &rows {
                        let s = &self.subs.entries[i];
                        if t >= s.start && t <= s.end {
                            chosen = Some((i, false));
                            break;
                        }
                    }
                    if chosen.is_none() {
                        for &i in &rows {
                            if self.subs.entries[i].start > t {
                                chosen = Some((i, true));
                                break;
                            }
                        }
                    }
                    let (idx, needs_seek) = chosen.unwrap_or((rows[0], true));
                    self.playback_cursor = Some(idx);
                    if needs_seek {
                        self.seek_both(self.subs.entries[idx].start);
                        t = self.target.position();
                    }
                }

                if self.last_logic_seek.elapsed() > Duration::from_millis(250) {
                    if let Some(cur) = self.playback_cursor {
                        let (start, end) = {
                            let s = &self.subs.entries[cur];
                            (s.start, s.end)
                        };
                        if t >= end - 0.02 {
                            let next = rows.iter().copied().find(|&i| i > cur).or_else(|| {
                                if self.loop_selected {
                                    rows.first().copied()
                                } else {
                                    None
                                }
                            });
                            if let Some(idx) = next {
                                self.playback_cursor = Some(idx);
                                self.seek_both(self.subs.entries[idx].start);
                                t = self.target.position();
                            } else if !self.loop_selected {
                                self.pause();
                            }
                        } else if t < start - 0.02 {
                            self.seek_both(start);
                            t = self.target.position();
                        }
                    }
                }
            }
        }

        if self.has_reference
            && self.last_ref_sync.elapsed() > Duration::from_millis(350)
            && self.reference.path.is_some()
        {
            let desired = (t + self.baseline).max(0.0);
            if (self.reference.position() - desired).abs() > 0.25 {
                self.reference.seek(desired);
            }
            self.last_ref_sync = Instant::now();
        }
    }

    // ---------------------------------------------------------------- video UI

    fn video_panel(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        is_target: bool,
        title: &str,
        placeholder: &str,
    ) {
        let player = if is_target { &self.target } else { &self.reference };
        let frame = player.frame();
        let path_text = player
            .path
            .as_ref()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
            .unwrap_or_else(|| placeholder.to_string());

        egui::Frame::new()
            .inner_margin(8.0)
            .corner_radius(10.0)
            .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(title).size(15.0).strong()));
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(path_text).size(12.0).weak());
                    });
                });
                ui.add_space(4.0);

                // Smaller previews leave more room for the editor below.
                let width = ui.available_width().max(120.0);
                let height = (width * 0.42).clamp(130.0, 300.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());

                if let Some(frame) = frame {
                    // Re-upload pixels only when a new frame arrives. Previously this
                    // cloned + re-uploaded the full RGB buffer (several MB) every frame.
                    let version = if is_target {
                        self.target.frame_version()
                    } else {
                        self.reference.frame_version()
                    };
                    let (stored_ver, slot) = if is_target {
                        (&mut self.target_frame_ver, &mut self.target_tex)
                    } else {
                        (&mut self.ref_frame_ver, &mut self.ref_tex)
                    };
                    if *stored_ver != version || slot.is_none() {
                        let image =
                            egui::ColorImage::from_rgb([frame.width, frame.height], &frame.rgb);
                        match slot {
                            Some(handle) => handle.set(image, TextureOptions::LINEAR),
                            None => {
                                *slot = Some(ctx.load_texture("video_frame", image, TextureOptions::LINEAR));
                            }
                        }
                        *stored_ver = version;
                    }
                    let tex_id = slot.as_ref().expect("texture initialized").id();
                    let scale = (rect.width() / frame.width as f32)
                        .min(rect.height() / frame.height as f32);
                    let draw_size = Vec2::new(frame.width as f32 * scale, frame.height as f32 * scale);
                    let draw_rect = Rect::from_center_size(rect.center(), draw_size);
                    ui.painter().image(
                        tex_id,
                        draw_rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );

                    if is_target {
                        let t = self.target.position();
                        if let Some(idx) = self.subs.active_at(t) {
                            if let Some(sub) = self.subs.entries.get(idx) {
                                let text = sub.text.replace('\n', " ");
                                let font = FontId::new(
                                    self.settings.preview_font_size,
                                    FontFamily::Proportional,
                                );
                                let color = Color32::from_rgba_unmultiplied(
                                    (self.settings.preview_color[0] * 255.0) as u8,
                                    (self.settings.preview_color[1] * 255.0) as u8,
                                    (self.settings.preview_color[2] * 255.0) as u8,
                                    (self.settings.preview_color[3] * 255.0) as u8,
                                );
                                let painter = ui.painter();
                                let galley = painter.layout(
                                    text,
                                    font,
                                    color,
                                    (draw_rect.width() - 24.0).max(40.0),
                                );
                                let pos = egui::pos2(
                                    draw_rect.center().x - galley.size().x / 2.0,
                                    draw_rect.bottom() - galley.size().y - 14.0,
                                );
                                let bg = Rect::from_min_size(
                                    pos - Vec2::new(8.0, 6.0),
                                    galley.size() + Vec2::new(16.0, 12.0),
                                );
                                painter.rect_filled(bg, 6.0, Color32::from_black_alpha(150));
                                painter.galley(pos, galley, color);
                            }
                        }
                    }
                } else {
                    let painter = ui.painter();
                    painter.rect_filled(rect, 6.0, Color32::from_gray(60));
                    let loading = if is_target {
                        self.target_loading_since
                    } else {
                        self.reference_loading_since
                    };
                    let (text, color) = if let Some(since) = loading {
                        // Animated elapsed counter so the user can see work is
                        // in progress while the pipeline prerolls its first frame.
                        let e = since.elapsed();
                        let dots = ".".repeat(((e.as_millis() / 400) % 4 + 1) as usize);
                        (
                            format!(
                                "{} {:02}/{:02}/{:02}.{:03}{}",
                                self.lang().loading_video,
                                e.as_secs() / 3600,
                                (e.as_secs() / 60) % 60,
                                e.as_secs() % 60,
                                e.subsec_millis(),
                                dots
                            ),
                            rgb(230, 170, 60),
                        )
                    } else {
                        (placeholder.to_string(), Color32::from_gray(160))
                    };
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        text,
                        FontId::new(14.0, FontFamily::Proportional),
                        color,
                    );
                }
            });
    }

    // ------------------------------------------------------------- toolbar

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let s = self.lang();
        ui.horizontal(|ui| {
            if icon_button(ui, ico::VIDEO_CAMERA, s.load_target, 18.0) {
                if let Some(path) = Self::pick_video() {
                    self.request_action(PendingAction::LoadTarget(path));
                }
            }
            if icon_button(ui, ico::FILM_SLATE, s.load_reference, 18.0) {
                if let Some(path) = Self::pick_video() {
                    self.request_action(PendingAction::LoadReference(path));
                }
            }
            if self.has_reference && icon_button(ui, ico::X, s.unload_reference, 17.0) {
                self.unload_reference();
            }
            ui.separator();
            if icon_button(ui, ico::FILE_TEXT, s.load_subtitle, 18.0) {
                self.request_action(PendingAction::LoadSubtitle);
            }
            if icon_button(ui, ico::FILE_PLUS, s.new_subtitle, 18.0) {
                self.request_action(PendingAction::NewSubtitle);
            }
            if icon_button(ui, ico::FLOPPY_DISK, s.save_subtitle, 18.0) {
                self.save_subtitle(false);
            }
            if icon_button(ui, ico::EXPORT, s.save_subtitle_as, 18.0) {
                self.save_subtitle(true);
            }
            ui.separator();
            if icon_button(ui, ico::GEAR, s.open_settings, 18.0) {
                self.settings_open = true;
            }
            if icon_button(ui, ico::INFO, s.about, 18.0) {
                self.about_open = true;
            }
        });
    }

    // ------------------------------------------------------------- control bar

    fn controls(&mut self, ui: &mut egui::Ui) {
        let s = self.lang();
        let pal = palette_for(self.settings.color_scheme);
        let ctx = ui.ctx().clone();
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(10, 6))
            .corner_radius(10.0)
            .fill(pal.faint)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    // Transport icons
                    if icon_button(ui, ico::SKIP_BACK, s.goto_start, 15.0) {
                        self.pause();
                        self.seek_both(0.0);
                        self.show_seeked_frame(&ctx);
                    }
                    if icon_button(ui, ico::REWIND, s.step_back, 15.0) {
                        self.pause();
                        let t = (self.target.position() - self.settings.step_seconds).max(0.0);
                        self.seek_both(t);
                        self.show_seeked_frame(&ctx);
                    }
                    let (play_icon, play_tip) = if self.playing {
                        (ico::PAUSE, s.pause)
                    } else {
                        (ico::PLAY, s.play)
                    };
                    if icon_button(ui, play_icon, play_tip, 17.0) {
                        if self.playing { self.pause() } else { self.play() }
                    }
                    if icon_button(ui, ico::STOP, s.stop, 15.0) {
                        self.stop();
                    }
                    if icon_button(ui, ico::FAST_FORWARD, s.step_forward, 15.0) {
                        self.pause();
                        let t = self.target.position() + self.settings.step_seconds;
                        self.seek_both(t);
                        self.show_seeked_frame(&ctx);
                    }
                    if icon_button(ui, ico::SKIP_FORWARD, s.goto_next, 15.0) {
                        self.pause();
                        if let Some(idx) = self.next_playback_after(self.target.position()) {
                            self.seek_both(self.subs.entries[idx].start);
                        }
                        self.show_seeked_frame(&ctx);
                    }
                    ui.separator();

                    // Playback mode toggle icons
                    if ui
                        .add(
                            egui::Button::new(RichText::new(ico::PLAYLIST).size(15.0))
                                .selected(self.play_mode == PlayMode::Normal)
                                .min_size(Vec2::new(30.0, 28.0)),
                        )
                        .on_hover_text(s.mode_normal)
                        .clicked()
                    {
                        self.play_mode = PlayMode::Normal;
                        self.loop_selected = false;
                        self.playback_cursor = None;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new(ico::SUBTITLES).size(15.0))
                                .selected(self.play_mode == PlayMode::SubtitleOnly)
                                .min_size(Vec2::new(30.0, 28.0)),
                        )
                        .on_hover_text(s.mode_subtitle)
                        .clicked()
                    {
                        self.play_mode = PlayMode::SubtitleOnly;
                        self.loop_selected = false;
                        self.playback_cursor = None;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new(ico::REPEAT).size(15.0))
                                .selected(self.loop_selected)
                                .min_size(Vec2::new(30.0, 28.0)),
                        )
                        .on_hover_text(s.loop_selected)
                        .clicked()
                    {
                        self.loop_selected = !self.loop_selected;
                        if self.loop_selected {
                            self.play_mode = PlayMode::Normal;
                        }
                        self.playback_cursor = None;
                    }
                    ui.separator();

                    // Audio source cycling icon
                    let (audio_icon, audio_tip) = match self.audio_source {
                        AudioSource::Muted => (ico::SPEAKER_SLASH, s.audio_mute),
                        AudioSource::Target => (ico::SPEAKER_HIGH, s.audio_target),
                        AudioSource::Reference => (ico::FILM_SLATE, s.audio_reference),
                    };
                    let audio_clicked = icon_button(ui, audio_icon, audio_tip, 15.0);
                    if matches!(self.audio_source, AudioSource::Reference) && !self.has_reference {
                        ui.label(RichText::new("⚠").color(rgb(230, 170, 60)));
                    }
                    if audio_clicked {
                        self.audio_source = match self.audio_source {
                            AudioSource::Muted => AudioSource::Target,
                            AudioSource::Target => AudioSource::Reference,
                            AudioSource::Reference => AudioSource::Muted,
                        };
                        self.apply_audio_source();
                    }
                    ui.separator();

                    // Baseline + step controls
                    ui.label(RichText::new(s.baseline).size(12.0));
                    let before = self.baseline;
                    ui.add(
                        DragValue::new(&mut self.baseline)
                            .speed(0.05)
                            .range(-600.0..=600.0)
                            .suffix(" s"),
                    );
                    if icon_button(ui, ico::MINUS, "−0.1 s", 13.0) {
                        self.baseline -= 0.1;
                    }
                    if icon_button(ui, ico::PLUS, "+0.1 s", 13.0) {
                        self.baseline += 0.1;
                    }
                    if icon_button(ui, ico::ARROW_COUNTER_CLOCKWISE, s.baseline_reset, 14.0) {
                        self.baseline = 0.0;
                    }
                    if (self.baseline - before).abs() > 1e-6 {
                        self.on_baseline_changed(&ctx);
                    }
                    ui.label(RichText::new(s.step_duration).size(12.0));
                    ui.add(
                        DragValue::new(&mut self.settings.step_seconds)
                            .speed(0.1)
                            .range(0.05..=30.0)
                            .suffix(" s"),
                    );
                    ui.separator();

                    // Timeline spans from the start-time label to the end-time label.
                    let raw_duration = self.target.duration;
                    let duration = raw_duration.max(0.1);
                    let mut pos = self.target.position().clamp(0.0, duration);
                    let time_w = 78.0;
                    let slider_w = (ui.available_width() - time_w * 2.0 - 12.0).max(80.0);
                    ui.label(RichText::new(srt::format_time(pos)).size(12.0));
                    // egui's Slider asks for `spacing.slider_width` (200px) by
                    // default, so `add_sized` alone cannot stretch it. Override
                    // the width for this one widget to fill the remaining row.
                    let prev_slider_width = ui.spacing().slider_width;
                    ui.spacing_mut().slider_width = slider_w;
                    let response = ui.add(
                        egui::Slider::new(&mut pos, 0.0..=duration).show_value(false),
                    );
                    ui.spacing_mut().slider_width = prev_slider_width;
                    if response.drag_started() {
                        self.pause();
                    }
                    if response.changed() {
                        self.seek_both(pos);
                        // Seek while paused must still decode a fresh frame in
                        // both previews, just like the transport buttons.
                        self.show_seeked_frame(&ctx);
                    }
                    ui.label(RichText::new(srt::format_time(raw_duration)).size(12.0));

                    // Floating hint follows the pointer while hovering or
                    // dragging, showing the pointed time and its percentage.
                    let active = response.hovered() || response.is_pointer_button_down_on();
                    if active && response.rect.width() > 1.0 {
                        if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                            let frac =
                                ((p.x - response.rect.left()) / response.rect.width()).clamp(0.0, 1.0);
                            let hint = format!(
                                "{}  {:.1}%",
                                srt::format_time((frac * raw_duration as f32) as f64),
                                frac * 100.0
                            );
                            let font = FontId::new(12.0, FontFamily::Monospace);
                            let galley = ui.painter().layout_no_wrap(hint, font, pal.text);
                            let box_w = (galley.size().x + 12.0).min(response.rect.width());
                            let box_h = galley.size().y + 8.0;
                            let x = (p.x - box_w / 2.0).clamp(
                                response.rect.left(),
                                (response.rect.right() - box_w).max(response.rect.left()),
                            );
                            let above = response.rect.top() - box_h - 4.0;
                            let y = if above >= 0.0 {
                                above
                            } else {
                                response.rect.bottom() + 4.0
                            };
                            let hint_rect =
                                Rect::from_min_size(pos2(x, y), Vec2::new(box_w, box_h));
                            ui.painter().rect_filled(
                                hint_rect,
                                CornerRadius::same(4),
                                pal.faint,
                            );
                            ui.painter().galley(
                                pos2(hint_rect.left() + 6.0, hint_rect.top() + 4.0),
                                galley,
                                pal.text,
                            );
                        }
                    }
                });
            });
    }

    // ------------------------------------------------------------ subtitles UI

    fn time_cell(
        &mut self,
        ui: &mut egui::Ui,
        row: usize,
        is_start: bool,
        value: f64,
        pal: &Palette,
        font_size: f32,
    ) -> (Option<f64>, bool) {
        let mut changed = None;
        let mut selected = false;
        let bg = if is_start { pal.start_btn } else { pal.end_btn };
        let text_color = if is_start { pal.time_start } else { pal.time_end };
        let key = (row, is_start);
        let mut buffer = self
            .time_buffers
            .entry(key)
            .or_insert_with(|| srt::format_time(value))
            .clone();

        // The chosen timestamp stays visibly highlighted; its colour comes
        // from the existing start/end palette, so no scheme changes are needed.
        let frame = if self.active_time == Some(key) {
            egui::Frame::new()
                .fill(bg)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .corner_radius(CornerRadius::same(4))
        } else {
            egui::Frame::new()
        };
        // A full `00:00:00,000` stamp needs roughly 7.4em in a monospace font.
        let time_width = font_size * 7.4 + 12.0;

        let resp = ui.add(
            egui::TextEdit::singleline(&mut buffer)
                .id(Id::unique(("time_field", row, is_start)))
                .desired_width(time_width)
                .frame(frame)
                .text_color(text_color)
                .font(FontId::new(font_size, FontFamily::Monospace)),
        );
        // Picking a timestamp arms the shared fine-tune buttons and selects its row.
        if resp.clicked() || resp.gained_focus() {
            self.active_time = Some(key);
            selected = true;
        }
        if resp.changed() {
            if let Some(t) = srt::parse_time(&buffer) {
                changed = Some(t);
            }
        }
        if resp.lost_focus() && srt::parse_time(&buffer).is_none() {
            buffer = srt::format_time(value);
        }

        self.time_buffers.insert(key, buffer);
        (changed, selected)
    }

    fn subtitle_editor(&mut self, ui: &mut egui::Ui) {
        let s = self.lang();
        let pal = palette_for(self.settings.color_scheme);
        let ctx = ui.ctx().clone();

        ui.horizontal(|ui| {
            ui.add(egui::Label::new(RichText::new(s.subtitle_panel).size(16.0).strong()));
            if self.subs.dirty {
                ui.label(RichText::new(s.unsaved).color(rgb(230, 170, 60)).size(12.0));
            }
            if !self.selected_rows.is_empty() {
                ui.label(
                    RichText::new(format!("{}", self.selected_rows.len()))
                        .color(pal.time_start)
                        .size(12.0),
                );
            }
            ui.separator();
            if icon_button(ui, ico::CHECK_SQUARE, s.select_all, 14.0) {
                self.select_all_rows();
            }
            if icon_button(ui, ico::X_SQUARE, s.deselect_all, 14.0) {
                self.clear_selection();
            }
            if icon_button(ui, ico::MAGNIFYING_GLASS, s.find_replace, 14.0) {
                self.find_open = !self.find_open;
            }
            ui.separator();
            // Shared time fine-tune buttons: inert until a timestamp is picked.
            let time_ready = self.active_time.is_some();
            if icon_button_enabled(ui, ico::CARET_DOUBLE_LEFT, s.time_minus_1, 14.0, time_ready) {
                self.adjust_active_time(-1.0);
            }
            if icon_button_enabled(ui, ico::CARET_LEFT, s.time_minus_01, 14.0, time_ready) {
                self.adjust_active_time(-0.1);
            }
            if icon_button_enabled(ui, ico::CARET_RIGHT, s.time_plus_01, 14.0, time_ready) {
                self.adjust_active_time(0.1);
            }
            if icon_button_enabled(ui, ico::CARET_DOUBLE_RIGHT, s.time_plus_1, 14.0, time_ready) {
                self.adjust_active_time(1.0);
            }
            ui.separator();
            // Row actions sit with the other editor tools on the left.
            if icon_button(ui, ico::SCISSORS, s.cut, 14.0) {
                self.cut_selected();
            }
            if icon_button(ui, ico::COPY, s.copy, 14.0) {
                self.copy_selected();
            }
            if icon_button(ui, ico::CLIPBOARD, s.paste, 14.0) {
                self.paste_rows();
            }
            if icon_button(ui, ico::TRASH, s.del_row, 14.0) {
                self.delete_selected();
            }
            if icon_button(ui, ico::PLUS, s.add_row, 15.0) {
                self.add_row();
            }
        });
        ui.add_space(2.0);

        if self.find_open {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(s.find_replace).size(12.0).strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.find_text)
                        .hint_text(s.find_placeholder)
                        .desired_width(160.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.replace_text)
                        .hint_text(s.replace_placeholder)
                        .desired_width(160.0),
                );
                if icon_button(ui, ico::ARROW_RIGHT, s.find_next, 14.0) {
                    self.find_next(&ctx);
                }
                if icon_button(ui, ico::ARROW_CLOCKWISE, s.replace_one, 14.0) {
                    self.replace_current(&ctx);
                }
                if icon_button(ui, ico::ARROWS_CLOCKWISE, s.replace_all, 14.0) {
                    let count = self.replace_all();
                    if count == 0 {
                        self.set_status(s.find_not_found);
                    } else {
                        self.set_status(format!("{}: {}", s.replace_all, count));
                    }
                }
                if icon_button(ui, ico::X, s.close, 14.0) {
                    self.find_open = false;
                }
            });
            ui.add_space(2.0);
        }

        if self.subs.is_empty() {
            ui.label(RichText::new(s.no_subtitle_loaded).weak());
            return;
        }

        let editor_font = self.settings.editor_font_size;
        let len = self.subs.entries.len();
        let row_height = (editor_font + 10.0).max(26.0);

        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
        if ui.input(|i| i.pointer.any_released()) {
            self.block_dragging = false;
            self.block_anchor = None;
        }

        // show_rows only builds widgets for visible lines. Without this a
        // full-movie SRT (1000+ rows) creates tens of thousands of widgets
        // every frame and makes the UI unusably slow.
        let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(row) = self.pending_scroll_row.take() {
            // Place the found row a couple of lines below the top of the view.
            let target = (row as f32 * row_height - row_height * 2.0).max(0.0);
            scroll = scroll.vertical_scroll_offset(target);
        }
        scroll.show_rows(ui, row_height, len, |ui, row_range| {
                for i in row_range {
                    let start = self.subs.entries[i].start;
                    let end = self.subs.entries[i].end;
                    let mut text = self.subs.entries[i].text.clone();
                    let mut new_start = None;
                    let mut new_end = None;
                    let mut line_clicked = false;
                    let mut text_clicked = false;
                    let mut click_shift = false;
                    let mut click_ctrl = false;
                    let mut selected_by_time = false;

                    ui.horizontal(|ui| {
                        let row_rect = ui.max_rect();
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.set_height(row_height);

                        // A subtle full-row wash makes block selection visibly
                        // cover both the timestamps and the subtitle text.
                        if self.selected_rows.contains(&i) {
                            ui.painter().rect_filled(
                                row_rect.shrink(1.0),
                                CornerRadius::same(3),
                                pal.selected.gamma_multiply(0.30),
                            );
                        }

                        // Selection checkbox — `selected_rows` is the single
                        // source of truth, so text/line selection checks this too.
                        let mut checked = self.selected_rows.contains(&i);
                        let cb = ui.checkbox(&mut checked, "").on_hover_text(s.select_row);
                        if cb.changed() {
                            let shift = ui.input(|inp| inp.modifiers.shift);
                            let ctrl = ui.input(|inp| inp.modifiers.ctrl || inp.modifiers.command);
                            if shift {
                                self.select_range(i);
                            } else if ctrl {
                                self.select_toggle(i);
                            } else if checked {
                                self.select_only(i);
                            } else {
                                self.selected_rows.remove(&i);
                                if self.anchor_row == Some(i) {
                                    self.anchor_row = None;
                                }
                            }
                        }

                        // Line number gutter: click to toggle, drag for a block select.
                        let is_sel = self.selected_rows.contains(&i);
                        let line = ui.add(
                            egui::Button::new(
                                RichText::new(format!("{:>3}", i + 1))
                                    .color(if is_sel { pal.text } else { pal.line_no })
                                    .font(FontId::new(editor_font, FontFamily::Monospace)),
                            )
                            .sense(egui::Sense::click_and_drag())
                            .fill(if is_sel { pal.selected } else { Color32::TRANSPARENT })
                            .min_size(Vec2::new(40.0, row_height))
                            .corner_radius(CornerRadius::same(3)),
                        );
                        if line.drag_started() {
                            self.block_dragging = true;
                            self.block_anchor = Some(i);
                            self.select_only(i);
                        }
                        if self.block_dragging {
                            if let (Some(anchor), Some(p)) = (self.block_anchor, pointer_pos) {
                                if row_rect.contains(p) {
                                    self.select_range_from(anchor, i);
                                }
                            }
                        }
                        if line.clicked() {
                            line_clicked = true;
                            click_shift = ui.input(|inp| inp.modifiers.shift);
                            click_ctrl = ui.input(|inp| inp.modifiers.ctrl || inp.modifiers.command);
                        }

                        // Colour-coded timestamps, same size as the subtitle text.
                        let (start_change, start_selected) =
                            self.time_cell(ui, i, true, start, &pal, editor_font);
                        new_start = start_change;
                        selected_by_time |= start_selected;
                        let (end_change, end_selected) =
                            self.time_cell(ui, i, false, end, &pal, editor_font);
                        new_end = end_change;
                        selected_by_time |= end_selected;

                        // Subtitle text — frameless and flexible, like a text document.
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .id(Id::unique(("subtitle_text", i)))
                                .desired_width(f32::INFINITY)
                                .frame(egui::Frame::new())
                                .text_color(pal.sub_text)
                                .font(FontId::new(editor_font, FontFamily::Proportional)),
                        );
                        if resp.clicked() || resp.gained_focus() {
                            text_clicked = true;
                            click_shift = ui.input(|inp| inp.modifiers.shift);
                            click_ctrl = ui.input(|inp| inp.modifiers.ctrl || inp.modifiers.command);
                        }
                        if resp.changed() {
                            self.subs.entries[i].text = text;
                            self.subs.dirty = true;
                        }
                    });

                    if line_clicked {
                        // Gutter clicks toggle directly — no Shift required.
                        if click_shift {
                            self.select_range(i);
                        } else {
                            self.select_toggle(i);
                        }
                    } else if text_clicked {
                        if click_shift {
                            self.select_range(i);
                        } else if click_ctrl {
                            self.select_toggle(i);
                        } else {
                            self.select_only(i);
                        }
                    } else if selected_by_time {
                        self.select_only(i);
                    }
                    if (line_clicked || text_clicked || selected_by_time)
                        && !self.playing
                        && self.selected_rows.len() == 1
                        && self.selected_rows.contains(&i)
                    {
                        self.seek_both(self.subs.entries[i].start);
                        // Show the segment's first frame in both previews while paused.
                        self.show_seeked_frame(&ctx);
                    }

                    if (new_start.is_some() || new_end.is_some()) && i < self.subs.entries.len() {
                        if let Some(v) = new_start {
                            self.subs.entries[i].start = v.min(self.subs.entries[i].end);
                        }
                        if let Some(v) = new_end {
                            self.subs.entries[i].end = v.max(self.subs.entries[i].start);
                        }
                        self.subs.dirty = true;
                    }
                }
            });
    }

    // ------------------------------------------------------------------ dialogs

    fn settings_window(&mut self, ctx: &Context) {
        let s = self.lang();
        let before = self.settings.clone();
        let mut open = self.settings_open;
        let mut close_clicked = false;
        egui::Window::new(s.settings_window)
            .open(&mut open)
            .resizable(true)
            .default_size(Vec2::new(560.0, 560.0))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .spacing(Vec2::new(16.0, 12.0))
                    .show(ui, |ui| {
                        ui.label(s.language);
                        egui::ComboBox::from_id_salt("language_combo")
                            .selected_text(self.settings.language.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.settings.language, Language::Zh, "中文");
                                ui.selectable_value(&mut self.settings.language, Language::En, "English");
                            });
                        ui.end_row();

                        ui.label(s.theme);
                        egui::ComboBox::from_id_salt("scheme_combo")
                            .selected_text(scheme_label(self.settings.color_scheme, s))
                            .show_ui(ui, |ui| {
                                for scheme in [
                                    ColorScheme::Light,
                                    ColorScheme::Dark,
                                    ColorScheme::System,
                                    ColorScheme::Sepia,
                                    ColorScheme::Nord,
                                    ColorScheme::HighContrast,
                                ] {
                                    ui.selectable_value(
                                        &mut self.settings.color_scheme,
                                        scheme,
                                        scheme_label(scheme, s),
                                    );
                                }
                            });
                        ui.end_row();

                        ui.label(s.font_global);
                        ui.add(DragValue::new(&mut self.settings.global_font_size).range(8.0..=30.0).speed(0.5));
                        ui.end_row();

                        ui.label(s.font_editor);
                        ui.add(DragValue::new(&mut self.settings.editor_font_size).range(10.0..=40.0).speed(0.5));
                        ui.end_row();

                        ui.label(s.font_preview);
                        ui.add(DragValue::new(&mut self.settings.preview_font_size).range(10.0..=72.0).speed(0.5));
                        ui.end_row();

                        ui.label(s.preview_color);
                        {
                            let mut color = [
                                (self.settings.preview_color[0] * 255.0) as u8,
                                (self.settings.preview_color[1] * 255.0) as u8,
                                (self.settings.preview_color[2] * 255.0) as u8,
                                (self.settings.preview_color[3] * 255.0) as u8,
                            ];
                            if ui.color_edit_button_srgba_unmultiplied(&mut color).changed() {
                                self.settings.preview_color = [
                                    color[0] as f32 / 255.0,
                                    color[1] as f32 / 255.0,
                                    color[2] as f32 / 255.0,
                                    color[3] as f32 / 255.0,
                                ];
                            }
                        }
                        ui.end_row();

                        ui.label(s.step_duration);
                        ui.add(DragValue::new(&mut self.settings.step_seconds).range(0.05..=30.0).speed(0.1).suffix(" s"));
                        ui.end_row();

                        ui.label(s.font_family);
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.settings.font_path).desired_width(250.0));
                            if ui.button(s.browse).clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Font", &["ttf", "otf", "ttc"])
                                    .pick_file()
                                {
                                    self.settings.font_path = path.to_string_lossy().to_string();
                                }
                            }
                        });
                        ui.end_row();
                    });
                ui.add_space(2.0);
                ui.label(RichText::new(s.font_system_hint).weak().size(12.0));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(s.reset_defaults).clicked() {
                        self.settings = Settings::default();
                        self.applied_font = None;
                    }
                    if ui.button(s.close).clicked() {
                        close_clicked = true;
                    }
                });
            });
        self.settings_open = open && !close_clicked;
        // Only touch the disk when a setting actually changed (was: every frame).
        if self.settings != before {
            self.settings.save();
        }
    }

    fn about_dialog(&mut self, ctx: &Context) {
        if !self.about_open {
            return;
        }
        let s = self.lang();
        let mut open = true;
        let mut close_clicked = false;
        egui::Window::new(s.about_title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    ui.label(RichText::new(s.app_title).size(18.0).strong());
                    ui.label(RichText::new(s.about_intro).weak().size(12.0));
                });
                ui.add_space(10.0);
                ui.separator();
                egui::Grid::new("about_grid")
                    .num_columns(2)
                    .spacing(Vec2::new(14.0, 8.0))
                    .show(ui, |ui| {
                        ui.label(s.about_version);
                        ui.label(env!("CARGO_PKG_VERSION"));
                        ui.end_row();
                        ui.label(s.about_author);
                        ui.label("sinbud2004");
                        ui.end_row();
                        ui.label(s.about_email);
                        ui.hyperlink_to("sinbud2004@gmail.com", "mailto:sinbud2004@gmail.com");
                        ui.end_row();
                        ui.label(s.about_license);
                        ui.label("MIT License");
                        ui.end_row();
                        ui.label(s.about_source);
                        ui.hyperlink_to(
                            "github.com/Paul-sinbud2004/lambo-subtitle-edit",
                            "https://github.com/Paul-sinbud2004/lambo-subtitle-edit",
                        );
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    if ui.button(s.close).clicked() {
                        close_clicked = true;
                    }
                });
            });
        self.about_open = open && !close_clicked;
    }

    fn error_dialog(&mut self, ctx: &Context) {
        let s = self.lang();
        if let Some(msg) = self.error_message.clone() {
            let mut open = true;
            let mut acknowledged = false;
            egui::Window::new(s.error_title)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(&msg);
                    ui.add_space(8.0);
                    ui.label(RichText::new(s.decode_error_hint).weak());
                    ui.add_space(10.0);
                    if ui.button(s.ok).clicked() {
                        acknowledged = true;
                    }
                });
            if acknowledged || !open {
                self.error_message = None;
            }
        }
    }

    fn confirm_dialog(&mut self, ctx: &Context) {
        if self.pending.is_none() || !self.subs.dirty {
            return;
        }
        let s = self.lang();
        let mut open = true;
        egui::Window::new(s.confirm_title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(s.status_discard_changes);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button(s.ok).clicked() {
                        self.subs.dirty = false;
                        self.execute_pending();
                    }
                    if ui.button(s.cancel).clicked() {
                        self.pending = None;
                    }
                });
            });
        if !open {
            self.pending = None;
        }
    }
}

impl eframe::App for SubtitleEditor {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Rebuilding all styles/theme/fonts every frame is very expensive; only
        // do it when the relevant settings actually change.
        let sig = self.visual_signature();
        if self.applied_visual.as_ref() != Some(&sig) {
            self.apply_settings(&ctx);
            self.applied_visual = Some(sig);
        }
        self.run_initial_load();
        self.poll_video();
        self.tick_playback();
        self.handle_shortcuts(&ctx);

        egui::CentralPanel::default().show(ui, |ui| {
            self.toolbar(ui);
            ui.separator();

            let s = self.lang();
            ui.columns(2, |columns| {
                self.video_panel(&mut columns[0], &ctx, true, s.target_video, s.no_video_target);
                self.video_panel(&mut columns[1], &ctx, false, s.reference_video, s.no_video_ref);
            });

            ui.add_space(6.0);
            self.controls(ui);
            ui.add_space(6.0);
            self.subtitle_editor(ui);

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.status).weak().size(12.0));
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(path) = self.subs.path.as_ref() {
                        ui.label(RichText::new(path.display().to_string()).weak().size(11.0));
                    }
                });
            });
        });

        self.settings_window(&ctx);
        self.about_dialog(&ctx);
        self.error_dialog(&ctx);
        self.confirm_dialog(&ctx);

        if self.playing {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
}
