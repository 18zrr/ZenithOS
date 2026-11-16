//! Web Browser application for Zenith OS
//!
//! This module provides basic web browsing functionality with support for 
//! navigation, rendering and user interaction.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// Browser error types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BrowserError {
    /// Invalid URL provided
    InvalidUrl,
    /// Network connectivity issue
    NetworkError,
    /// Page loading failed
    LoadFailed,
}

/// Web browser structure
pub struct Browser {
    // Current browsing state
    current_url: String,
    history: Vec<String>,
}

impl Browser {
    /// Create a new browser instance
    pub fn new() -> Self {
        Self {
            current_url: "about:blank".to_string(),
            history: vec![],
        }
    }

    /// Navigate to a URL
    pub fn navigate(&mut self, url: &str) -> Result<(), BrowserError> {
        // Validate URL format
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(BrowserError::InvalidUrl);
        }
        
        // In a real implementation this would fetch and render the page.
        // Remember the page we are leaving so `go_back` can restore it.
        if self.current_url != "about:blank" {
            let previous = core::mem::take(&mut self.current_url);
            self.history.push(previous);
        }
        self.current_url = url.to_string();
        Ok(())
    }

    /// Go back in browsing history, restoring the previous URL.
    pub fn go_back(&mut self) -> Option<String> {
        let previous = self.history.pop()?;
        self.current_url = previous;
        Some(self.current_url.clone())
    }

    /// Reload current page
    pub fn reload(&self) -> Result<(), BrowserError> {
        // In a real implementation this would re-fetch the current page
        Ok(())
    }

    /// Get current URL
    pub fn get_current_url(&self) -> &str {
        &self.current_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_urls() {
        let mut browser = Browser::new();
        assert_eq!(browser.navigate("ftp://example.com"), Err(BrowserError::InvalidUrl));
        assert_eq!(browser.get_current_url(), "about:blank");
    }

    #[test]
    fn go_back_restores_previous_url() {
        let mut browser = Browser::new();
        browser.navigate("https://a.example").unwrap();
        browser.navigate("https://b.example").unwrap();
        assert_eq!(browser.go_back(), Some("https://a.example".to_string()));
        assert_eq!(browser.get_current_url(), "https://a.example");
    }
}