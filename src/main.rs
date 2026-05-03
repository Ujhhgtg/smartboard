mod app;
mod assets;
mod render;
mod state;
mod ui;
mod utils;

use std::backtrace::Backtrace;

use winit::event_loop::{ControlFlow, EventLoop};

#[cfg(not(target_os = "android"))]
fn main() {
    #[cfg(target_os = "linux")]
    utils::linux::silence_glib_logs();

    std::panic::set_hook(Box::new(|info| {
        eprintln!("panic: {info}");
        eprintln!("backtrace:\n{}", Backtrace::force_capture());

        rfd::MessageDialog::new()
            .set_title("应用崩溃")
            .set_level(rfd::MessageLevel::Error)
            .set_description(info.to_string())
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }));

    println!(
        r"
          __  ___      ____  __
         / / / / | /| / / / / /
        / /_/ /| |/ |/ / /_/ / 
        \__,_/ |__/|__/\__,_/  
    "
    );
    println!(
        "
    \x1b[3mujhhgtg's whiteboard, unleashed\x1b[0m
    "
    );

    pollster::block_on(run_desktop());
}

enum UserEvent {
    TrayIconEvent(tray_icon::TrayIconEvent),
}

#[cfg(not(target_os = "android"))]
async fn run_desktop() {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    let proxy = event_loop.create_proxy();
    tray_icon::TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::TrayIconEvent(event));
    }));
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = app::App::new();
    event_loop.run_app(&mut app).expect("failed to run app");
}
