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

async fn run() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = app::App::new();
    event_loop.run_app(&mut app).expect("Failed to run app");
}
