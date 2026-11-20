# Lambo Subtitle Edit

[中文](README.md)

An SRT subtitle editor with dual video preview, built with Rust + egui.

![Icon](assets/icon.png)

## Features

- Target video and optional reference video side by side, sharing transport, seeking and timeline controls; the reference has a configurable baseline offset.
- Clicking a subtitle row seeks both previews to that subtitle's start frame.
- IDE-style editor: line numbers, coloured timestamps/text, fine-tune time buttons, multi-select and cut/copy/paste.
- Play subtitle segments only, or loop the checked subtitles; both videos start muted and one audio route can be selected.
- Find and replace (next / current / all), Chinese and English UI, multiple colour schemes and font sizes.
- Subtitles auto-load from a matching filename, or can be loaded/created/saved manually; decode errors are reported in a dialog.

## Usage

```bash
# Start the editor
lambo-subtitle-edit

# Open a target video (and its matching subtitle) plus an optional reference video
lambo-subtitle-edit target.mp4 reference.mp4
```

The toolbar icons, left to right: load target, load reference, unload reference, load subtitle, new subtitle, save, save as, settings, about. Hover for tooltips.

- Click a subtitle row to select it and seek to its start; Shift-click for a range, or drag over line numbers for multi-select.
- Fine-tune buttons (`-1 s / -0.1 s / +0.1 s / +1 s`) apply to a timestamp you have clicked first.
- Use Preferences to change language, colour scheme, font sizes and preview text colour.

## Build

Requires Rust (stable), GStreamer runtime libraries and codec plugins.

```bash
cargo build --release
cargo test
./target/release/lambo-subtitle-edit
```

On Arch Linux: `yay -S lambo-subtitle-edit`.

## License

MIT © 2026 sinbud2004. Author email: sinbud2004@gmail.com
