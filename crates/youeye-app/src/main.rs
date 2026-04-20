mod app;
mod canvas;
mod export;
mod menu;
mod menu_linux;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod menu_native;
mod modifiers;
mod paths;
mod ui;

use anyhow::Result;
use tracing_subscriber::EnvFilter;
use winit::event_loop::EventLoop;

use crate::app::{App, UserEvent};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let proxy = event_loop.create_proxy();
    let mut app = App::new(proxy);
    event_loop.run_app(&mut app)?;
    Ok(())
}
