use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Subtitle {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

impl Subtitle {
    pub fn new(start: f64, end: f64, text: impl Into<String>) -> Self {
        Self {
            start,
            end,
            text: text.into(),
        }
    }
}

#[derive(Default)]
pub struct SubtitleFile {
    pub entries: Vec<Subtitle>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
}

impl SubtitleFile {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Index of the subtitle active at time `t` (in seconds), if any.
    pub fn active_at(&self, t: f64) -> Option<usize> {
        self.entries
            .iter()
            .position(|s| t >= s.start && t <= s.end)
    }

    /// Index of the first subtitle starting strictly after `t`.
    #[allow(dead_code)]
    pub fn next_after(&self, t: f64) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (i, s) in self.entries.iter().enumerate() {
            if s.start > t + 1e-6 && best.is_none_or(|(_, bt)| s.start < bt) {
                best = Some((i, s.start));
            }
        }
        best.map(|(i, _)| i)
    }
}

/// Parse a SRT timestamp like `00:01:02,345` (also accepts `.` and missing hours).
pub fn parse_time(input: &str) -> Option<f64> {
    let token = input.split_whitespace().next()?;
    let token = token.replace(',', ".");
    let parts: Vec<&str> = token.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut total = 0.0;
    for part in parts {
        let v: f64 = part.parse().ok()?;
        total = total * 60.0 + v;
    }
    if total.is_finite() && total >= 0.0 {
        Some(total)
    } else {
        None
    }
}

/// Format seconds as `HH:MM:SS,mmm`.
pub fn format_time(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    format!("{:02}:{:02}:{:02},{:03}", h, m, s, ms)
}

pub fn parse_srt(input: &str) -> Vec<Subtitle> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in normalized.lines() {
        if line.trim().is_empty() {
            if !current.trim().is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.trim().is_empty() {
        blocks.push(current);
    }

    let mut out = Vec::new();
    for block in blocks {
        let lines: Vec<&str> = block.lines().collect();
        let Some(arrow_idx) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let (start_s, end_s) = match lines[arrow_idx].split_once("-->") {
            Some(v) => v,
            None => continue,
        };
        let (Some(start), Some(end)) = (parse_time(start_s), parse_time(end_s)) else {
            continue;
        };
        let text = lines[arrow_idx + 1..].join("\n");
        out.push(Subtitle { start, end, text });
    }
    out
}

pub fn serialize_srt(entries: &[Subtitle]) -> String {
    let mut out = String::new();
    for (i, sub) in entries.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            format_time(sub.start),
            format_time(sub.end),
            sub.text
        ));
    }
    out
}

/// Look for a sidecar subtitle next to a video file (same stem, `.srt`).
pub fn find_sidecar(video: &Path) -> Option<PathBuf> {
    let stem = video.file_stem()?;
    let parent = video.parent()?;
    for ext in ["srt", "SRT", "srt.txt"] {
        let candidate = parent.join(format!("{}.{}", stem.to_string_lossy(), ext));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_roundtrip() {
        assert_eq!(parse_time("00:01:02,345"), Some(62.345));
        assert_eq!(parse_time("01:02.5"), Some(62.5));
        assert_eq!(format_time(62.345), "00:01:02,345");
    }

    #[test]
    fn parse_and_serialize() {
        let srt = "1\n00:00:01,000 --> 00:00:02,500\nHello\nworld\n\n2\n00:00:03,000 --> 00:00:04,000\nBye\n";
        let entries = parse_srt(srt);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "Hello\nworld");
        assert_eq!(entries[1].start, 3.0);
        let out = serialize_srt(&entries);
        assert_eq!(parse_srt(&out).len(), 2);
    }
}
