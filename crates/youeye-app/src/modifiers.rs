//! Cross-platform modifier-key abstraction.
//!
//! The "command" key is ⌘ on macOS and Ctrl everywhere else. Callers always
//! ask for [`Modifier::Command`]; never hardcode Ctrl.

#![allow(dead_code)] // Used from phase 2 onwards (keyboard-shortcut dispatch).

use egui::Modifiers;

/// Semantic modifier combinations. Resolve to platform-appropriate keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    /// "Primary" modifier — ⌘ on macOS, Ctrl elsewhere. Used for Save, Copy,
    /// Undo, etc.
    Command,
    Shift,
    Alt,
    /// Command + Shift together.
    CommandShift,
}

/// True if the given semantic modifier matches the current egui input.
pub fn held(mods: &Modifiers, m: Modifier) -> bool {
    match m {
        Modifier::Command => mods.command,
        Modifier::Shift => mods.shift,
        Modifier::Alt => mods.alt,
        Modifier::CommandShift => mods.command && mods.shift,
    }
}

/// Human-readable shortcut label for status text, tooltips, etc.
///
/// Renders "⌘S" on macOS and "Ctrl+S" on other platforms.
pub fn label(m: Modifier, key: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        match m {
            Modifier::Command => format!("⌘{key}"),
            Modifier::Shift => format!("⇧{key}"),
            Modifier::Alt => format!("⌥{key}"),
            Modifier::CommandShift => format!("⇧⌘{key}"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        match m {
            Modifier::Command => format!("Ctrl+{key}"),
            Modifier::Shift => format!("Shift+{key}"),
            Modifier::Alt => format!("Alt+{key}"),
            Modifier::CommandShift => format!("Ctrl+Shift+{key}"),
        }
    }
}
