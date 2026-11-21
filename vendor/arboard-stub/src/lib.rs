//! Minimal arboard stub for OHOS.
//!
//! The real `arboard` crate provides cross-platform clipboard access.
//! On OHOS, clipboard operations go through the system IME/input method.
//! This stub provides the type surface so downstream code compiles.

/// A text clipboard image (RGBA).
#[allow(dead_code)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

impl ImageData {
    pub fn new(width: u32, height: u32, bytes: Vec<u8>) -> Self {
        Self { width, height, bytes }
    }
}

/// The clipboard.
#[allow(dead_code)]
pub struct Clipboard {
    _private: (),
}

impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        Ok(Self { _private: () })
    }

    pub fn read_text(&mut self) -> Result<Option<String>, Error> {
        Ok(None)
    }

    pub fn write_text<S: AsRef<str>>(&mut self, _text: S) -> Result<(), Error> {
        Ok(())
    }

    pub fn read_image(&mut self) -> Result<Option<ImageData>, Error> {
        Ok(None)
    }

    pub fn write_image(&mut self, _image: ImageData) -> Result<(), Error> {
        Ok(())
    }
}

/// Clipboard error.
#[derive(Debug)]
pub struct Error {
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}
