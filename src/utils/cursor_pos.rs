pub type Point = winit::dpi::PhysicalPosition<f64>;

#[derive(Debug)]
pub enum CursorPosError {
    #[allow(unused)]
    Unsupported(&'static str),
    Os(&'static str),
}

impl std::fmt::Display for CursorPosError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CursorPosError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            CursorPosError::Os(msg) => write!(f, "os error: {msg}"),
        }
    }
}

impl std::error::Error for CursorPosError {}

pub fn current() -> Result<Point, CursorPosError> {
    platform::current_cursor_position()
}

#[cfg(target_os = "windows")]
mod platform {
    use windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetPhysicalCursorPos};

    use super::{CursorPosError, Point};

    pub fn current_cursor_position() -> Result<Point, CursorPosError> {
        let mut point = POINT { x: 0, y: 0 };

        let success = unsafe { GetPhysicalCursorPos(&mut point) }.is_ok();

        if success {
            Ok(Point {
                x: point.x as f64,
                y: point.y as f64,
            })
        } else {
            Err(CursorPosError::Os("GetPhysicalCursorPos failed"))
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{CursorPosError, Point};
    use core::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    type CGEventRef = *mut c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *const c_void) -> CGEventRef;
        fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
        fn CFRelease(cf: *const c_void);
    }

    pub fn current_cursor_position() -> Result<Point, CursorPosError> {
        unsafe {
            let event = CGEventCreate(core::ptr::null());
            if event.is_null() {
                return Err(CursorPosError::Os("CGEventCreate returned null"));
            }

            let loc = CGEventGetLocation(event);
            CFRelease(event as *const c_void);

            Ok(Point { x: loc.x, y: loc.y })
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{CursorPosError, Point};
    use once_cell::sync::Lazy;
    use std::env;
    use std::sync::Mutex;

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;

    struct X11State {
        conn: RustConnection,
        root: u32,
    }

    static X11: Lazy<Result<Mutex<X11State>, CursorPosError>> = Lazy::new(|| {
        if env::var_os("DISPLAY").is_none() {
            return Err(CursorPosError::Unsupported("DISPLAY not set"));
        }

        let (conn, screen_num) =
            RustConnection::connect(None).map_err(|_| CursorPosError::Os("X11 connect failed"))?;

        let root = conn.setup().roots[screen_num].root;

        Ok(Mutex::new(X11State { conn, root }))
    });

    pub fn current_cursor_position() -> Result<Point, CursorPosError> {
        if env::var_os("DISPLAY").is_some() {
            return current_cursor_position_x11();
        }

        if env::var_os("WAYLAND_DISPLAY").is_some() {
            return Err(CursorPosError::Unsupported(
                "native Wayland does not expose a global cursor position",
            ));
        }

        Err(CursorPosError::Unsupported(
            "neither DISPLAY nor WAYLAND_DISPLAY is set",
        ))
    }

    fn current_cursor_position_x11() -> Result<Point, CursorPosError> {
        let x11 = X11
            .as_ref()
            .map_err(|_| CursorPosError::Os("X11 init failed"))?;

        let guard = x11
            .lock()
            .map_err(|_| CursorPosError::Os("X11 mutex poisoned"))?;

        let reply = guard
            .conn
            .query_pointer(guard.root)
            .map_err(|_| CursorPosError::Os("XQueryPointer request failed"))?
            .reply()
            .map_err(|_| CursorPosError::Os("XQueryPointer reply failed"))?;

        Ok(Point {
            x: reply.root_x as f64,
            y: reply.root_y as f64,
        })
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    use super::{CursorPosError, Point};

    pub fn current_cursor_position() -> Result<Point, CursorPosError> {
        Err(CursorPosError::Unsupported("unsupported target OS"))
    }
}
