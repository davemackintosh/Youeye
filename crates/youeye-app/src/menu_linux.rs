//! Linux menu bar: drawn inside the egui frame as a top panel.
//!
//! Chosen over muda because Linux has no consistent menu-bar convention across
//! desktop environments and muda's Linux backend pulls GTK, which we don't
//! want as a dependency.

use egui::{Key, KeyboardShortcut, Modifiers};
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::app::UserEvent;
use crate::menu::{MenuAction, MenuBar};

pub struct EguiMenuBar;

impl EguiMenuBar {
    pub fn new() -> Self {
        Self
    }
}

impl MenuBar for EguiMenuBar {
    fn attach(&mut self, _window: &Window, _proxy: &EventLoopProxy<UserEvent>) {
        // No-op on Linux.
    }

    fn draw_egui(&mut self, ctx: &egui::Context, actions: &mut Vec<MenuAction>) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    item(
                        ui,
                        "New Project",
                        sc(Modifiers::COMMAND, Key::N),
                        MenuAction::NewProject,
                        actions,
                    );
                    item(
                        ui,
                        "Open Project…",
                        sc(Modifiers::COMMAND, Key::O),
                        MenuAction::OpenProject,
                        actions,
                    );
                    ui.separator();
                    item(
                        ui,
                        "Save",
                        sc(Modifiers::COMMAND, Key::S),
                        MenuAction::Save,
                        actions,
                    );
                    item(
                        ui,
                        "Save As…",
                        sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::S),
                        MenuAction::SaveAs,
                        actions,
                    );
                    ui.separator();
                    item(
                        ui,
                        "Quit",
                        sc(Modifiers::COMMAND, Key::Q),
                        MenuAction::Quit,
                        actions,
                    );
                });
                ui.menu_button("Edit", |ui| {
                    item(
                        ui,
                        "Undo",
                        sc(Modifiers::COMMAND, Key::Z),
                        MenuAction::Undo,
                        actions,
                    );
                    item(
                        ui,
                        "Redo",
                        sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
                        MenuAction::Redo,
                        actions,
                    );
                    ui.separator();
                    item(
                        ui,
                        "Cut",
                        sc(Modifiers::COMMAND, Key::X),
                        MenuAction::Cut,
                        actions,
                    );
                    item(
                        ui,
                        "Copy",
                        sc(Modifiers::COMMAND, Key::C),
                        MenuAction::Copy,
                        actions,
                    );
                    item(
                        ui,
                        "Paste",
                        sc(Modifiers::COMMAND, Key::V),
                        MenuAction::Paste,
                        actions,
                    );
                    item(
                        ui,
                        "Duplicate",
                        sc(Modifiers::COMMAND, Key::D),
                        MenuAction::Duplicate,
                        actions,
                    );
                    ui.separator();
                    item(
                        ui,
                        "Select All",
                        sc(Modifiers::COMMAND, Key::A),
                        MenuAction::SelectAll,
                        actions,
                    );
                });
                ui.menu_button("View", |ui| {
                    item(
                        ui,
                        "Zoom In",
                        sc(Modifiers::COMMAND, Key::Plus),
                        MenuAction::ZoomIn,
                        actions,
                    );
                    item(
                        ui,
                        "Zoom Out",
                        sc(Modifiers::COMMAND, Key::Minus),
                        MenuAction::ZoomOut,
                        actions,
                    );
                    ui.separator();
                    item(
                        ui,
                        "Zoom to Fit",
                        sc(Modifiers::COMMAND, Key::Num0),
                        MenuAction::ZoomToFit,
                        actions,
                    );
                    item(
                        ui,
                        "Actual Size",
                        sc(Modifiers::COMMAND, Key::Num1),
                        MenuAction::ZoomActual,
                        actions,
                    );
                });
                ui.menu_button("Help", |ui| {
                    item(ui, "About youeye", None, MenuAction::About, actions);
                });
            });
        });
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn handle_native_event(&self, _event: &muda::MenuEvent, _actions: &mut Vec<MenuAction>) {
        // Unreachable on Linux; present only to satisfy the trait signature on
        // cfg-combined builds.
    }
}

fn sc(modifiers: Modifiers, key: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(modifiers, key))
}

fn item(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: Option<KeyboardShortcut>,
    action: MenuAction,
    actions: &mut Vec<MenuAction>,
) {
    let mut button = egui::Button::new(label);
    if let Some(s) = shortcut {
        button = button.shortcut_text(ui.ctx().format_shortcut(&s));
    }
    if ui.add(button).clicked() {
        actions.push(action);
        ui.close();
    }
}
