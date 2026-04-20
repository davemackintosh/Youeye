//! Menu-bar abstraction with platform-split implementations.
//!
//! - macOS + Windows: native menus via `muda` (see [`menu_native`]).
//! - Linux: in-window egui menu bar (see [`menu_linux`]), because Linux has no
//!   consistent top-of-screen menu convention across desktop environments.
//!
//! The rest of the app only talks to the [`MenuBar`] trait and reacts to
//! [`MenuAction`]s — it never knows which impl is live.

use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::app::UserEvent;

/// Semantic menu commands. Keep platform-agnostic; never put key codes or
/// platform-specific hooks in here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    // File
    NewProject,
    OpenProject,
    Save,
    SaveAs,
    ExportPng,
    Quit,
    // Edit
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Duplicate,
    SelectAll,
    // View
    ZoomIn,
    ZoomOut,
    ZoomToFit,
    ZoomActual,
    // Help
    About,
}

pub trait MenuBar {
    /// One-time attach after the main window exists. Native impls wire the
    /// menu to the `NSApplication` / `HWND`; the Linux impl is a no-op.
    fn attach(&mut self, window: &Window, proxy: &EventLoopProxy<UserEvent>);

    /// Draws the menu bar inside the egui frame. Native impls are a no-op.
    fn draw_egui(&mut self, ctx: &egui::Context, actions: &mut Vec<MenuAction>);

    /// Called when the event loop forwards a native menu event. Native impls
    /// decode it to a [`MenuAction`]; the Linux impl is a no-op.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn handle_native_event(&self, event: &muda::MenuEvent, actions: &mut Vec<MenuAction>);
}

/// Build the menu bar implementation for the current platform.
pub fn create() -> Box<dyn MenuBar> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        Box::new(crate::menu_native::NativeMenuBar::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Box::new(crate::menu_linux::EguiMenuBar::new())
    }
}
