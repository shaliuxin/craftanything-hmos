//! Minimal eframe compatibility layer for HarmonyOS.
//!
//! The real `eframe` crate provides a windowed app framework on top of egui + winit.
//! On OHOS we drive egui directly via wgpu (GLES), so this stub only provides the
//! **type surface** that downstream crates reference (`eframe::App`, `eframe::Frame`,
//! `eframe::Storage`). No windowing, no winit, no accesskit — just the traits.

/// An opaque frame handle. The real eframe `Frame` carries window/renderer access,
/// but `CraftApp::ui()` never dereferences it (`_frame`), so an empty struct suffices.
#[derive(Debug, Clone, Default)]
pub struct Frame;

impl Frame {
    /// For API compat; always returns a no-op frame on OHOS.
    #[allow(dead_code)]
    pub fn new(_egui_ctx: egui::Context) -> Self {
        Self
    }
}

/// Storage backend trait. The real eframe implements this with JSON files;
/// on OHOS we back it with the app sandbox.
pub trait Storage {
    /// Get a string value by key.
    fn get_string(&self, key: &str) -> Option<String>;
    /// Set a string value.
    fn set_string(&self, key: &str, val: String);
}

/// The app trait that downstream crates implement.
pub trait App {
    /// Draw one frame.
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame);

    /// Save persistent state (called on window close).
    fn save(&mut self, _storage: &mut dyn Storage) {}

    /// Called when the window is being closed.
    fn on_exit(&mut self) {}
}
