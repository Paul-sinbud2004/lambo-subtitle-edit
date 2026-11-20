use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::{AppSink, AppSinkCallbacks};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A decoded RGB video frame ready to be uploaded as an egui texture.
#[derive(Clone)]
pub struct VideoFrame {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

/// Thin wrapper around a GStreamer `playbin` with an `appsink` video sink.
/// Decoding is performed by whatever GStreamer plugins are installed on the
/// system, so no codecs are bundled with the application.
pub struct VideoPlayer {
    pipeline: gst::Pipeline,
    _appsink: AppSink,
    frame: Arc<Mutex<Option<Arc<VideoFrame>>>>,
    frame_version: Arc<AtomicU64>,
    errors: Arc<Mutex<Vec<String>>>,
    eos: Arc<Mutex<bool>>,
    bus: Option<gst::Bus>,
    pub path: Option<PathBuf>,
    pub duration: f64,
}

impl VideoPlayer {
    pub fn new() -> Result<Self, String> {
        let pipeline = gst::ElementFactory::make("playbin")
            .build()
            .map_err(|e| format!("无法创建 playbin: {e}"))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| "playbin 不是 GStreamer Pipeline".to_string())?;

        let frame: Arc<Mutex<Option<Arc<VideoFrame>>>> = Arc::new(Mutex::new(None));
        let frame_version: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
        let errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let eos: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGB")
            .build();

        let frame_cb = frame.clone();
        let version_cb = frame_version.clone();
        let callbacks = AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                let Ok(sample) = sink.pull_sample() else {
                    return Err(gst::FlowError::Eos);
                };
                let Some(buffer) = sample.buffer() else {
                    return Ok(gst::FlowSuccess::Ok);
                };
                let Some(caps) = sample.caps() else {
                    return Ok(gst::FlowSuccess::Ok);
                };
                let Some(s) = caps.structure(0) else {
                    return Ok(gst::FlowSuccess::Ok);
                };
                let (Ok(width), Ok(height)) = (s.get::<i32>("width"), s.get::<i32>("height")) else {
                    return Ok(gst::FlowSuccess::Ok);
                };
                let (width, height) = (width as usize, height as usize);
                if width == 0 || height == 0 {
                    return Ok(gst::FlowSuccess::Ok);
                }

                let map = buffer.map_readable();
                let Ok(map) = map else {
                    return Ok(gst::FlowSuccess::Ok);
                };
                let bytes: &[u8] = map.as_ref();
                let needed = width * 3;
                let stride = bytes.len() / height;
                if stride >= needed {
                    let mut rgb = Vec::with_capacity(needed * height);
                    for y in 0..height {
                        let start = y * stride;
                        rgb.extend_from_slice(&bytes[start..start + needed]);
                    }
                    if let Ok(mut guard) = frame_cb.lock() {
                        *guard = Some(Arc::new(VideoFrame {
                            width,
                            height,
                            rgb,
                        }));
                        version_cb.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Ok(gst::FlowSuccess::Ok)
            })
            .build();

        let appsink = AppSink::builder()
            .caps(&caps)
            .sync(true)
            .drop(true)
            .max_buffers(1)
            .callbacks(callbacks)
            .build();

        pipeline.set_property("video-sink", appsink.upcast_ref::<gst::Element>());
        pipeline.set_property("mute", true);

        let bus = pipeline.bus();
        Ok(Self {
            pipeline,
            _appsink: appsink,
            frame,
            frame_version,
            errors,
            eos,
            bus,
            path: None,
            duration: 0.0,
        })
    }

    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        self.stop();
        if let Ok(mut f) = self.frame.lock() {
            *f = None;
        }
        if let Ok(mut e) = self.errors.lock() {
            e.clear();
        }
        if let Ok(mut b) = self.eos.lock() {
            *b = false;
        }

        let canonical = path
            .canonicalize()
            .map_err(|e| format!("无法定位文件: {e}"))?;
        let uri = format!("file://{}", canonical.to_string_lossy());

        self.pipeline.set_property("uri", uri.as_str());
        self.pipeline
            .set_state(gst::State::Paused)
            .map(|_| ())
            .map_err(|e| format!("无法打开视频: {e}"))?;
        self.path = Some(path.to_path_buf());
        self.duration = 0.0;
        Ok(())
    }

    pub fn unload(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        self.path = None;
        self.duration = 0.0;
        if let Ok(mut f) = self.frame.lock() {
            *f = None;
        }
    }

    /// Retry querying the duration until the pipeline has pre-rolled.
    pub fn refresh_duration(&mut self) {
        if self.path.is_some() && self.duration <= 0.0 {
            if let Some(d) = self.pipeline.query_duration::<gst::ClockTime>() {
                let secs = d.nseconds() as f64 / 1_000_000_000.0;
                if secs > 0.0 && secs.is_finite() {
                    self.duration = secs;
                }
            }
        }
    }

    pub fn position(&self) -> f64 {
        self.pipeline
            .query_position::<gst::ClockTime>()
            .map(|p| p.nseconds() as f64 / 1_000_000_000.0)
            .unwrap_or(0.0)
    }

    pub fn play(&mut self) -> Result<(), String> {
        self.pipeline
            .set_state(gst::State::Playing)
            .map(|_| ())
            .map_err(|e| format!("无法播放: {e}"))
    }

    pub fn pause(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Paused);
    }

    pub fn stop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }

    pub fn seek(&mut self, seconds: f64) -> bool {
        if self.path.is_none() {
            return false;
        }
        let t = seconds.max(0.0);
        let flags = gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE;
        self.pipeline
            .seek_simple(flags, gst::ClockTime::from_seconds_f64(t))
            .is_ok()
    }

    pub fn set_muted(&self, muted: bool) {
        self.pipeline.set_property("mute", muted);
    }

    /// Cheap clone of the shared frame (no pixel copy).
    pub fn frame(&self) -> Option<Arc<VideoFrame>> {
        self.frame.lock().ok()?.clone()
    }

    /// Monotonically increasing counter, bumped whenever a new frame arrives.
    pub fn frame_version(&self) -> u64 {
        self.frame_version.load(Ordering::Relaxed)
    }

    /// Drain error / EOS messages from the bus synchronously. eframe does not
    /// run a GLib main loop, so a bus watch would never fire.
    pub fn poll_bus(&self) {
        let Some(bus) = &self.bus else { return };
        let types = [gst::MessageType::Error, gst::MessageType::Eos];
        while let Some(msg) = bus.pop_filtered(&types) {
            match msg.view() {
                gst::MessageView::Error(err) => {
                    if let Ok(mut v) = self.errors.lock() {
                        v.push(err.error().to_string());
                    }
                }
                gst::MessageView::Eos(..) => {
                    if let Ok(mut b) = self.eos.lock() {
                        *b = true;
                    }
                }
                _ => {}
            }
        }
    }

    pub fn take_errors(&self) -> Vec<String> {
        self.errors.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
    }

    pub fn take_eos(&self) -> bool {
        self.eos.lock().map(|mut b| std::mem::take(&mut *b)).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn decodes_frames_through_system_plugins() {
        let video = std::path::Path::new("/tmp/opencode/srt_editor_test.mp4");
        if !video.exists() {
            eprintln!("skipping: no sample video");
            return;
        }
        gst::init().unwrap();
        let mut player = VideoPlayer::new().unwrap();
        player.load(video).unwrap();
        player.play().unwrap();
        thread::sleep(Duration::from_millis(900));
        let frame = player.frame().expect("no decoded frame arrived");
        assert_eq!(frame.width, 640);
        assert_eq!(frame.height, 360);
        assert_eq!(frame.rgb.len(), 640 * 360 * 3);
        player.pause();
        assert!(player.position() > 0.0, "position did not advance");
        let errors = player.take_errors();
        assert!(errors.is_empty(), "decode errors: {errors:?}");
    }
}
