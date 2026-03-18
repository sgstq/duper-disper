use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A minimal recording overlay that shows a small indicator when recording.
/// Uses eframe for a transparent, always-on-top window.
pub struct RecordingOverlay {
    visible: Arc<AtomicBool>,
    status_text: Arc<std::sync::Mutex<String>>,
}

impl RecordingOverlay {
    pub fn new() -> Self {
        Self {
            visible: Arc::new(AtomicBool::new(false)),
            status_text: Arc::new(std::sync::Mutex::new(String::new())),
        }
    }

    pub fn show_recording(&self) {
        self.set_status("Recording...");
        self.visible.store(true, Ordering::SeqCst);
    }

    pub fn show_transcribing(&self) {
        self.set_status("Transcribing...");
    }

    pub fn show_refining(&self) {
        self.set_status("Refining...");
    }

    pub fn hide(&self) {
        self.visible.store(false, Ordering::SeqCst);
    }

    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    fn set_status(&self, text: &str) {
        *self.status_text.lock().unwrap() = text.to_string();
    }

    pub fn get_status(&self) -> String {
        self.status_text.lock().unwrap().clone()
    }
}
