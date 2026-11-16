//! System Settings application for Zenith OS
//!
//! This module provides the core functionality for system configuration and settings management.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// System settings error types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SystemSettingsError {
    /// Invalid setting name provided
    InvalidSetting,
    /// Permission denied when trying to change a setting
    PermissionDenied,
    /// Setting value is out of range or invalid format
    InvalidValue,
}

/// System settings structure
pub struct SystemSettings {
    // Settings storage would go here
    settings: Vec<(String, String)>,
}

impl SystemSettings {
    /// Create a new system settings instance
    pub fn new() -> Self {
        Self {
            settings: vec![],
        }
    }

    /// Get a setting value by name
    pub fn get_setting(&self, key: &str) -> Option<&String> {
        self.settings.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Set a setting value by name, creating it if it does not exist yet.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), SystemSettingsError> {
        if key.is_empty() {
            return Err(SystemSettingsError::InvalidSetting);
        }

        if let Some((_, stored)) = self.settings.iter_mut().find(|(k, _)| k == key) {
            *stored = value.to_string();
        } else {
            self.settings.push((key.to_string(), value.to_string()));
        }

        Ok(())
    }

    /// List all available settings
    pub fn list_settings(&self) -> Vec<String> {
        self.settings.iter().map(|(k, _)| k.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_and_update() {
        let mut settings = SystemSettings::new();
        settings.set_setting("theme", "dark").unwrap();
        assert_eq!(settings.get_setting("theme").map(|s| s.as_str()), Some("dark"));

        settings.set_setting("theme", "light").unwrap();
        assert_eq!(settings.get_setting("theme").map(|s| s.as_str()), Some("light"));
        assert_eq!(settings.list_settings(), vec!["theme".to_string()]);
    }

    #[test]
    fn empty_key_is_rejected() {
        let mut settings = SystemSettings::new();
        assert_eq!(
            settings.set_setting("", "value"),
            Err(SystemSettingsError::InvalidSetting)
        );
    }
}