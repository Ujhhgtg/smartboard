pub const ICON: &[u8] = include_bytes!("../assets/images/app_icon/icon.ico");

#[cfg(feature = "startup_animation")]
include!(concat!(env!("OUT_DIR"), "/startup_frames.rs"));
#[cfg(feature = "startup_animation")]
pub const STARTUP_AUDIO: &[u8] = include_bytes!("../assets/startup_animation/audio.wav");
