//! Native menu bar for macOS and Windows via `muda`.
//!
//! On macOS this installs an `NSMenu` on `NSApplication`; on Windows it
//! installs a menu on the `HWND`. Menu events arrive asynchronously through
//! muda's global channel; we bridge them into the winit event loop in
//! [`NativeMenuBar::attach`].

#![cfg(any(target_os = "macos", target_os = "windows"))]

use std::collections::HashMap;

use muda::{
    accelerator::{Accelerator, Code, Modifiers as MudaModifiers},
    Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::app::UserEvent;
use crate::menu::{MenuAction, MenuBar};

/// Cross-platform "primary" modifier — ⌘ on Mac, Ctrl on Windows.
#[cfg(target_os = "macos")]
const CMD: MudaModifiers = MudaModifiers::META;
#[cfg(target_os = "windows")]
const CMD: MudaModifiers = MudaModifiers::CONTROL;

pub struct NativeMenuBar {
    menu: Menu,
    items: HashMap<MenuId, MenuAction>,
}

impl NativeMenuBar {
    pub fn new() -> Self {
        let menu = Menu::new();
        let mut items = HashMap::<MenuId, MenuAction>::new();

        // macOS app menu — Apple HIG-compliant ordering: About, separator,
        // Services, separator, Hide/Hide Others/Show All, separator, Quit.
        #[cfg(target_os = "macos")]
        {
            let app = Submenu::new("youeye", true);
            app.append_items(&[
                &PredefinedMenuItem::about(Some("About youeye"), None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(None),
            ])
            .expect("build app menu");
            menu.append(&app).expect("append app menu");
        }

        // File
        let file = Submenu::new("&File", true);
        add(&file, "New Project", Some(accel(CMD, Code::KeyN)), MenuAction::NewProject, &mut items);
        add(&file, "Open Project…", Some(accel(CMD, Code::KeyO)), MenuAction::OpenProject, &mut items);
        file.append(&PredefinedMenuItem::separator()).unwrap();
        add(&file, "Save", Some(accel(CMD, Code::KeyS)), MenuAction::Save, &mut items);
        add(&file, "Save As…", Some(accel(CMD | MudaModifiers::SHIFT, Code::KeyS)), MenuAction::SaveAs, &mut items);
        #[cfg(not(target_os = "macos"))]
        {
            file.append(&PredefinedMenuItem::separator()).unwrap();
            add(&file, "Quit", Some(accel(CMD, Code::KeyQ)), MenuAction::Quit, &mut items);
        }
        menu.append(&file).unwrap();

        // Edit
        let edit = Submenu::new("&Edit", true);
        add(&edit, "Undo", Some(accel(CMD, Code::KeyZ)), MenuAction::Undo, &mut items);
        add(&edit, "Redo", Some(accel(CMD | MudaModifiers::SHIFT, Code::KeyZ)), MenuAction::Redo, &mut items);
        edit.append(&PredefinedMenuItem::separator()).unwrap();
        add(&edit, "Cut", Some(accel(CMD, Code::KeyX)), MenuAction::Cut, &mut items);
        add(&edit, "Copy", Some(accel(CMD, Code::KeyC)), MenuAction::Copy, &mut items);
        add(&edit, "Paste", Some(accel(CMD, Code::KeyV)), MenuAction::Paste, &mut items);
        add(&edit, "Duplicate", Some(accel(CMD, Code::KeyD)), MenuAction::Duplicate, &mut items);
        edit.append(&PredefinedMenuItem::separator()).unwrap();
        add(&edit, "Select All", Some(accel(CMD, Code::KeyA)), MenuAction::SelectAll, &mut items);
        menu.append(&edit).unwrap();

        // View
        let view = Submenu::new("&View", true);
        add(&view, "Zoom In", Some(accel(CMD, Code::Equal)), MenuAction::ZoomIn, &mut items);
        add(&view, "Zoom Out", Some(accel(CMD, Code::Minus)), MenuAction::ZoomOut, &mut items);
        view.append(&PredefinedMenuItem::separator()).unwrap();
        add(&view, "Zoom to Fit", Some(accel(CMD, Code::Digit0)), MenuAction::ZoomToFit, &mut items);
        add(&view, "Actual Size", Some(accel(CMD, Code::Digit1)), MenuAction::ZoomActual, &mut items);
        menu.append(&view).unwrap();

        // Help (Windows-only; macOS gets About in the app menu)
        #[cfg(not(target_os = "macos"))]
        {
            let help = Submenu::new("&Help", true);
            add(&help, "About youeye", None, MenuAction::About, &mut items);
            menu.append(&help).unwrap();
        }

        Self { menu, items }
    }
}

impl MenuBar for NativeMenuBar {
    fn attach(&mut self, window: &Window, proxy: &EventLoopProxy<UserEvent>) {
        // Bridge muda's global event channel to the winit event loop so menu
        // clicks wake the app even when it's idle.
        let proxy = proxy.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = proxy.send_event(UserEvent::MenuEvent(event));
        }));

        #[cfg(target_os = "macos")]
        {
            let _ = window;
            self.menu.init_for_nsapp();
        }
        #[cfg(target_os = "windows")]
        {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let handle = window.window_handle().expect("window handle");
            if let RawWindowHandle::Win32(h) = handle.as_raw() {
                unsafe {
                    self.menu
                        .init_for_hwnd(h.hwnd.get() as isize)
                        .expect("init menu for HWND");
                }
            }
        }
    }

    fn draw_egui(&mut self, _ctx: &egui::Context, _actions: &mut Vec<MenuAction>) {
        // No-op: menu is native and lives outside the egui frame.
    }

    fn handle_native_event(&self, event: &MenuEvent, actions: &mut Vec<MenuAction>) {
        if let Some(action) = self.items.get(&event.id).copied() {
            actions.push(action);
        }
    }
}

fn accel(modifiers: MudaModifiers, code: Code) -> Accelerator {
    Accelerator::new(Some(modifiers), code)
}

fn add(
    parent: &Submenu,
    label: &str,
    accelerator: Option<Accelerator>,
    action: MenuAction,
    items: &mut HashMap<MenuId, MenuAction>,
) {
    let item = MenuItem::new(label, true, accelerator);
    items.insert(item.id().clone(), action);
    parent.append(&item).expect("append menu item");
}
