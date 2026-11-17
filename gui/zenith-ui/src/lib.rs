//! Zenith UI - Custom Graphical Environment for Zenith OS

#![no_std]
#![crate_name = "zenith_ui"]
// Fields describe the planned desktop model and are consumed by the renderer
// once a framebuffer backend exists.
#![allow(dead_code)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// Windows 11-inspired dark palette shared by the UI model.
#[derive(Clone, Copy)]
pub struct Theme {
    pub wallpaper_top: u32,
    pub wallpaper_bottom: u32,
    pub window: u32,
    pub title_bar: u32,
    pub taskbar: u32,
    pub accent: u32,
    pub text: u32,
    pub muted: u32,
}

impl Theme {
    /// Fluent-style dark theme with a blue bloom and blue accent.
    pub const DARK: Self = Self {
        wallpaper_top: 0x000D1B33,
        wallpaper_bottom: 0x001B3D66,
        window: 0x001F1F1F,
        title_bar: 0x002B2B2B,
        taskbar: 0x00202020,
        accent: 0x000078D4,
        text: 0x00FFFFFF,
        muted: 0x009A9A9A,
    };
}

/// A real software RGB surface that the UI model renders into. The kernel's
/// framebuffer driver is independent; this is the crate's own render target.
pub struct Surface {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

impl Surface {
    /// Create a zeroed surface of the given size.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width.saturating_mul(height)],
        }
    }

    /// Return the surface size in pixels.
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Fill the entire surface with one colour.
    pub fn clear(&mut self, color: u32) {
        for pixel in self.pixels.iter_mut() {
            *pixel = color;
        }
    }

    /// Fill a clipped rectangle with one colour.
    pub fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: u32) {
        for row in y..y.saturating_add(height) {
            if row >= self.height {
                break;
            }
            for column in x..x.saturating_add(width) {
                if column >= self.width {
                    break;
                }
                self.pixels[row * self.width + column] = color;
            }
        }
    }

    /// Read a pixel, returning zero when out of bounds.
    pub fn get(&self, x: usize, y: usize) -> u32 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.pixels[y * self.width + x]
    }
}

/// Graphics display system abstraction 
pub struct DisplaySystem {
    width: u32,
    height: u32,
    color_depth: u8,
}

impl DisplaySystem {
    /// Initialize the graphics display
    pub fn new(width: u32, height: u32) -> Self {
        DisplaySystem {
            width,
            height,
            color_depth: 32, // ARGB format
        }
    }

    /// Clear screen with specified background color
    pub fn clear(&self, _color: u32) {
        // Implementation would clear framebuffer or graphics buffer
        // For now, just a placeholder that does nothing
    }

    /// Draw a pixel at given coordinates
    pub fn draw_pixel(&self, _x: u32, _y: u32, _color: u32) {
        // Placeholder for actual drawing implementation
    }

    /// Get screen dimensions
    pub fn get_dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Input device abstraction 
pub struct InputManager {
    mouse_x: i32,
    mouse_y: i32,
    mouse_buttons: u8,
}

impl InputManager {
    /// Create new input manager
    pub fn new() -> Self {
        InputManager {
            mouse_x: 0,
            mouse_y: 0,
            mouse_buttons: 0,
        }
    }

    /// Handle mouse movement 
    pub fn handle_mouse_move(&mut self, x: i32, y: i32) {
        self.mouse_x = x;
        self.mouse_y = y;
    }

    /// Handle mouse button press
    pub fn handle_mouse_press(&mut self, button: u8) {
        self.mouse_buttons |= button;
    }

    /// Handle mouse button release  
    pub fn handle_mouse_release(&mut self, button: u8) {
        self.mouse_buttons &= !button;
    }
}

/// Window management system
#[derive(Debug)]
pub struct Window {
    id: u32,
    title: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    is_visible: bool,
}

impl Window {
    /// Create a new window
    pub fn new(id: u32, title: &str, x: i32, y: i32, width: u32, height: u32) -> Self {
        Window {
            id,
            title: title.to_string(),
            x,
            y,
            width,
            height,
            is_visible: true,
        }
    }

    /// Show the window
    pub fn show(&mut self) {
        self.is_visible = true;
    }

    /// Hide the window  
    pub fn hide(&mut self) {
        self.is_visible = false;
    }

    /// Move window to new position
    pub fn move_to(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
    }
}

/// Taskbar implementation
pub struct Taskbar {
    height: u32,
    items: Vec<String>,
}

impl Taskbar {
    /// Create new taskbar 
    pub fn new() -> Self {
        Taskbar {
            height: 48,
            items: Vec::new(),
        }
    }

    /// Add application to taskbar
    pub fn add_item(&mut self, app_name: &str) {
        self.items.push(app_name.to_string());
    }

    /// Remove application from taskbar  
    pub fn remove_item(&mut self, app_name: &str) {
        self.items.retain(|item| item != app_name);
    }

    /// Return the taskbar height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
}

/// Dock implementation
pub struct Dock {
    height: u32,
    items: Vec<String>,
}

impl Dock {
    /// Create new dock 
    pub fn new() -> Self {
        Dock {
            height: 60,
            items: Vec::new(),
        }
    }

    /// Add application to dock
    pub fn add_item(&mut self, app_name: &str) {
        self.items.push(app_name.to_string());
    }
}

/// Start menu implementation  
pub struct StartMenu {
    is_open: bool,
    applications: Vec<String>,
}

impl StartMenu {
    /// Create new start menu 
    pub fn new() -> Self {
        StartMenu {
            is_open: false,
            applications: vec![
                "Terminal".to_string(),
                "File Manager".to_string(), 
                "System Settings".to_string(),
                "Calculator".to_string(),
                "Browser".to_string(),
                "Media Player".to_string(),
                "System Monitor".to_string(),
            ],
        }
    }

    /// Open the start menu
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Close the start menu
    pub fn close(&mut self) {
        self.is_open = false;
    }
}

/// Notification system
pub struct NotificationCenter {
    notifications: Vec<String>,
}

impl NotificationCenter {
    /// Create new notification center
    pub fn new() -> Self {
        NotificationCenter {
            notifications: Vec::new(),
        }
    }

    /// Add a notification  
    pub fn add_notification(&mut self, message: &str) {
        self.notifications.push(message.to_string());
    }

    /// Clear all notifications
    pub fn clear_notifications(&mut self) {
        self.notifications.clear();
    }
}

/// Lock screen implementation
pub struct LockScreen {
    is_locked: bool,
    time: String,
    date: String,
}

impl LockScreen {
    /// Create new lock screen
    pub fn new() -> Self {
        LockScreen {
            is_locked: false,
            time: "00:00".to_string(),
            date: "Monday, Jan 1".to_string(),
        }
    }

    /// Show lock screen
    pub fn show(&mut self) {
        self.is_locked = true;
    }

    /// Hide lock screen  
    pub fn hide(&mut self) {
        self.is_locked = false;
    }
}

/// Main Zenith UI manager
pub struct ZenithUIManager {
    display: DisplaySystem,
    surface: Surface,
    theme: Theme,
    input_manager: InputManager,
    windows: Vec<Window>,
    taskbar: Taskbar,
    dock: Dock,
    start_menu: StartMenu,
    notification_center: NotificationCenter,
    lock_screen: LockScreen,
}

impl ZenithUIManager {
    /// Create new Zenith UI manager
    pub fn new(width: u32, height: u32) -> Self {
        ZenithUIManager {
            display: DisplaySystem::new(width, height),
            surface: Surface::new(width as usize, height as usize),
            theme: Theme::DARK,
            input_manager: InputManager::new(),
            windows: Vec::new(),
            taskbar: Taskbar::new(),
            dock: Dock::new(),
            start_menu: StartMenu::new(),
            notification_center: NotificationCenter::new(),
            lock_screen: LockScreen::new(),
        }
    }

    /// Borrow the rendered surface for display or testing.
    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    /// Borrow the active theme.
    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// Borrow the taskbar model.
    pub fn taskbar(&self) -> &Taskbar {
        &self.taskbar
    }

    /// Borrow the Start menu model.
    pub fn start_menu(&self) -> &StartMenu {
        &self.start_menu
    }

    /// Initialize the UI system by clearing the surface.
    pub fn initialize(&mut self) {
        self.display.clear(self.theme.wallpaper_top);
        self.surface.clear(self.theme.wallpaper_top);
    }

    /// Render the complete desktop interface into the software surface.
    pub fn render(&mut self) {
        if self.lock_screen.is_locked {
            self.surface.clear(self.theme.wallpaper_top);
            return;
        }
        self.render_wallpaper();
        self.render_window();
        self.render_taskbar();
    }

    fn render_wallpaper(&mut self) {
        let (width, height) = self.surface.dimensions();
        for row in 0..height {
            let t = row * 100 / height.max(1);
            let top = self.theme.wallpaper_top;
            let bottom = self.theme.wallpaper_bottom;
            let channel = |left: u32, right: u32| -> u32 {
                let l = ((left >> 16) & 0xFF) as usize;
                let r = ((right >> 16) & 0xFF) as usize;
                ((l * (100 - t) + r * t) / 100) as u32
            };
            let red = channel(top, bottom);
            let green = {
                let l = ((top >> 8) & 0xFF) as usize;
                let r = ((bottom >> 8) & 0xFF) as usize;
                ((l * (100 - t) + r * t) / 100) as u32
            };
            let blue = {
                let l = (top & 0xFF) as usize;
                let r = (bottom & 0xFF) as usize;
                ((l * (100 - t) + r * t) / 100) as u32
            };
            let color = (red << 16) | (green << 8) | blue;
            self.surface.fill_rect(0, row, width, 1, color);
        }
    }

    fn render_window(&mut self) {
        let (width, height) = self.surface.dimensions();
        let x = width / 8;
        let y = height / 10;
        let w = width.saturating_sub(x * 2);
        let h = height.saturating_sub(y + self.taskbar.height as usize + 24);
        self.surface.fill_rect(x + 4, y + 6, w, h, 0x00081220);
        self.surface.fill_rect(x, y, w, h, self.theme.title_bar);
        self.surface.fill_rect(x + 1, y + 40, w.saturating_sub(2), h.saturating_sub(41), self.theme.window);
        self.surface.fill_rect(x + 14, y + 12, 16, 16, self.theme.accent);
        self.surface.fill_rect(x + 40, y + 16, 96, 8, self.theme.text);
    }

    fn render_taskbar(&mut self) {
        let (width, height) = self.surface.dimensions();
        let y = height.saturating_sub(self.taskbar.height as usize);
        self.surface.fill_rect(0, y, width, self.taskbar.height as usize, self.theme.taskbar);
        self.surface.fill_rect(0, y, width, 1, 0x003A3A3A);
        let icons = [0x000078D4, 0x00E3B341, 0x0071BA78, 0x00C586C0, 0x00D13438];
        let group_w = icons.len() * 48 + 48;
        let base = width.saturating_sub(group_w) / 2;
        for (index, color) in icons.iter().enumerate() {
            self.surface.fill_rect(base + 48 + index * 48 + 8, y + 8, 32, 32, *color);
        }
    }

    /// Internal rendering method for dock (data model only).
    fn render_dock(&self) {
        let _ = &self.dock;
    }

    /// Internal rendering method for windows (data model only).
    fn render_windows(&self) {
        let _ = &self.windows;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_clear_and_rect() {
        let mut surface = Surface::new(8, 8);
        surface.clear(0x111111);
        assert_eq!(surface.get(0, 0), 0x111111);
        surface.fill_rect(2, 2, 3, 3, 0x222222);
        assert_eq!(surface.get(3, 3), 0x222222);
        assert_eq!(surface.get(0, 0), 0x111111);
        assert_eq!(surface.get(8, 8), 0);
    }

    #[test]
    fn theme_has_fluent_palette() {
        assert_eq!(Theme::DARK.accent, 0x000078D4);
        assert_eq!(Theme::DARK.window, 0x001F1F1F);
    }

    #[test]
    fn manager_renders_taskbar() {
        let mut manager = ZenithUIManager::new(64, 48);
        manager.initialize();
        manager.render();
        // The taskbar occupies the bottom 48 rows of this small surface.
        assert_eq!(manager.surface().get(0, 47), Theme::DARK.taskbar);
    }
}