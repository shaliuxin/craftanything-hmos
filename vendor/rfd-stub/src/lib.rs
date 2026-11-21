//! Minimal rfd stub for OHOS.
//!
//! The real `rfd` crate provides native file open/save dialogs. On OHOS, file
//! selection is handled by the ArkTS `DocumentViewPicker` through the N-API bridge.
//! These types exist only so that downstream code referencing `rfd::*` compiles.

use std::path::PathBuf;

/// Dialog builder (compiles but does nothing on OHOS).
#[allow(dead_code, clippy::new_without_default)]
pub struct FileDialog {
    _private: (),
}

impl FileDialog {
    pub fn new() -> Self {
        Self { _private: () }
    }

    pub fn pick_file(&self) -> Option<PathBuf> {
        None
    }

    pub fn pick_folder(&self) -> Option<PathBuf> {
        None
    }

    pub fn pick_file_dialog(&self) -> Option<PathBuf> {
        None
    }

    pub fn save_file(&self) -> Option<PathBuf> {
        None
    }

    pub fn set_title<S: AsRef<str>>(&mut self, _title: S) -> &mut Self {
        self
    }

    pub fn add_filter<S: AsRef<str>>(&mut self, _name: S, _extensions: &[String]) -> &mut Self {
        self
    }
}

/// Async file dialog builder (no-op on OHOS; file selection goes through ArkTS).
#[allow(dead_code)]
pub struct AsyncFileDialog {
    _title: Option<String>,
    _file_name: Option<String>,
    _filters: Vec<(String, Vec<String>)>,
}

impl AsyncFileDialog {
    pub fn new() -> Self {
        Self { _title: None, _file_name: None, _filters: Vec::new() }
    }

    pub fn set_title<S: AsRef<str>>(&mut self, title: S) -> &mut Self {
        self._title = Some(title.as_ref().to_string());
        self
    }

    pub fn set_file_name<S: AsRef<str>>(&mut self, name: S) -> &mut Self {
        self._file_name = Some(name.as_ref().to_string());
        self
    }

    pub fn add_filter<S: AsRef<str>>(&mut self, name: S, extensions: &[impl AsRef<str>]) -> &mut Self {
        let _name = name.as_ref().to_string();
        let _exts: Vec<String> = extensions.iter().map(|e| e.as_ref().to_string()).collect();
        self._filters.push((_name, _exts));
        self
    }

    /// Pick a file — returns a future that resolves to None (no-op on OHOS).
    pub fn pick_file(&mut self) -> PickFileFuture {
        PickFileFuture { _resolved: false }
    }

    /// Pick multiple files (no-op on OHOS).
    pub fn pick_files(&mut self) -> PickFilesFuture {
        PickFilesFuture { _resolved: false }
    }

    /// Pick a folder (no-op on OHOS).
    pub fn pick_folder(&mut self) -> PickFileFuture {
        PickFileFuture { _resolved: false }
    }

    /// Save a file — returns a future that resolves to None (no-op on OHOS).
    pub fn save_file(&mut self) -> SaveFileFuture {
        SaveFileFuture { _resolved: false }
    }
}

/// A handle to a picked file (mimics rfd's FileHandle).
#[derive(Debug, Clone)]
pub struct FileHandle {
    pub path: PathBuf,
}

impl FileHandle {
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

/// Future returned by `pick_file` / `pick_folder` (resolves to None on OHOS).
#[allow(dead_code)]
pub struct PickFileFuture {
    _resolved: bool,
}

impl std::future::Future for PickFileFuture {
    type Output = Option<FileHandle>;
    fn poll(mut self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        self._resolved = true;
        std::task::Poll::Ready(None)
    }
}

/// Future returned by `pick_files` (resolves to None on OHOS).
#[allow(dead_code)]
pub struct PickFilesFuture {
    _resolved: bool,
}

impl std::future::Future for PickFilesFuture {
    type Output = Option<Vec<FileHandle>>;
    fn poll(mut self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        self._resolved = true;
        std::task::Poll::Ready(None)
    }
}

/// Future returned by `save_file` (resolves to None on OHOS).
#[allow(dead_code)]
pub struct SaveFileFuture {
    _resolved: bool,
}

impl std::future::Future for SaveFileFuture {
    type Output = Option<FileHandle>;
    fn poll(mut self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        self._resolved = true;
        std::task::Poll::Ready(None)
    }
}

/// Message dialog (no-op on OHOS).
#[allow(dead_code)]
pub enum MessageDialog {
    Show,
}

#[allow(dead_code)]
pub struct MessageDialogBuilder {
    _private: (),
}

impl MessageDialogBuilder {
    pub fn new() -> Self {
        Self { _private: () }
    }

    pub fn set_title<S: AsRef<str>>(&mut self, _title: S) -> &mut Self {
        self
    }

    pub fn set_description<S: AsRef<str>>(&mut self, _desc: S) -> &mut Self {
        self
    }

    pub fn show(&self) -> bool {
        false
    }
}
