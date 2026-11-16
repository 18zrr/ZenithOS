//! Media Player application for Zenith OS
//!
//! This module provides basic media playback functionality with support for 
//! audio and video files.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};

/// Media player error types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaPlayerError {
    /// Invalid file format or unsupported media type
    UnsupportedFormat,
    /// File not found
    FileNotFound,
    /// Playback operation failed
    PlaybackFailed,
}

/// Media player structure
pub struct MediaPlayer {
    // Current playback state
    is_playing: bool,
    current_file: Option<String>,
    volume: u8,
}

impl MediaPlayer {
    /// Create a new media player instance
    pub fn new() -> Self {
        Self {
            is_playing: false,
            current_file: None,
            volume: 50, // Default volume level
        }
    }

    /// Load and play a media file
    pub fn play(&mut self, filename: &str) -> Result<(), MediaPlayerError> {
        // Validate file exists and has supported format
        if !filename.ends_with(".mp3") && !filename.ends_with(".wav") && 
           !filename.ends_with(".mp4") && !filename.ends_with(".avi") {
            return Err(MediaPlayerError::UnsupportedFormat);
        }
        
        self.current_file = Some(filename.to_string());
        self.is_playing = true;
        Ok(())
    }

    /// Pause playback
    pub fn pause(&mut self) {
        self.is_playing = false;
    }

    /// Stop playback
    pub fn stop(&mut self) {
        self.is_playing = false;
        self.current_file = None;
    }

    /// Set volume level (0-100)
    pub fn set_volume(&mut self, volume: u8) -> Result<(), MediaPlayerError> {
        if volume > 100 {
            Err(MediaPlayerError::PlaybackFailed)
        } else {
            self.volume = volume;
            Ok(())
        }
    }

    /// Get current playback state
    pub fn is_playing(&self) -> bool {
        self.is_playing
    }

    /// Get current file name
    pub fn get_current_file(&self) -> Option<&str> {
        self.current_file.as_ref().map(|s| s.as_str())
    }
}