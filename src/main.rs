mod app;
mod i18n;
mod settings;
mod srt;
mod video;

use std::sync::Arc;

/// Embedded 128x128 RGBA application icon (see assets/icon.svg).
const APP_ICON: &[u8] = include_bytes!("../assets/icon.rgba");

fn main() -> eframe::Result<()> {
    gstreamer::init().expect("无法初始化 GStreamer，请检查系统是否安装了 GStreamer 运行库");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 960.0])
            .with_min_inner_size([960.0, 640.0])
            .with_title("蓝博字幕编辑器 · Lambo Subtitle Edit")
            .with_icon(Arc::new(egui::IconData {
                rgba: APP_ICON.to_vec(),
                width: 128,
                height: 128,
            })),
        ..Default::default()
    };

    let mut args = std::env::args_os().skip(1).map(std::path::PathBuf::from);
    let target = args.next();
    let reference = args.next();

    eframe::run_native(
        "lambo-subtitle-edit",
        options,
        Box::new(move |cc| {
            let mut editor = app::SubtitleEditor::new(cc);
            if let Some(path) = target {
                editor.open_initial(path, reference);
            }
            Ok(Box::new(editor))
        }),
    )
}
