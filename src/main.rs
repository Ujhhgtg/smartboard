#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod app;
mod render;
mod state;
mod utils;

use std::backtrace::Backtrace;

use winit::event_loop::{ControlFlow, EventLoop};

fn main() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("panic: {info}");
        eprintln!("backtrace:\n{}", Backtrace::force_capture());

        rfd::MessageDialog::new()
            .set_title("Application Panic")
            .set_level(rfd::MessageLevel::Error)
            .set_description(&info.to_string())
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }));

    #[cfg(not(target_arch = "wasm32"))]
    {
        pollster::block_on(run());
    }
}

enum UserEvent {
    TrayIconEvent(tray_icon::TrayIconEvent),
}

async fn run() {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    let proxy = event_loop.create_proxy();
    tray_icon::TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::TrayIconEvent(event));
    }));
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = app::App::new();
    event_loop.run_app(&mut app).expect("Failed to run app");
}
