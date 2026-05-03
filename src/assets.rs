use std::sync::OnceLock;

pub const ICON: &[u8] = include_bytes!("../assets/images/app_icon/icon.ico");

#[cfg(feature = "startup_animation")]
include!(concat!(env!("OUT_DIR"), "/startup_frames.rs"));
#[cfg(feature = "startup_animation")]
pub const STARTUP_AUDIO: &[u8] = include_bytes!("../assets/startup_animation/audio.wav");

#[cfg(feature = "embedded_font")]
pub const EMBEDDED_FONT: &[u8] = include_bytes!("../assets/fonts/noto-sans-cjk-sc-regular.otf");

pub fn font_bytes() -> &'static [u8] {
    static FONT: OnceLock<Vec<u8>> = OnceLock::new();

    FONT.get_or_init(|| {
        #[cfg(feature = "embedded_font")]
        {
            EMBEDDED_FONT.to_vec()
        }

        #[cfg(feature = "system_font")]
        {
            let mut font_db = fontdb::Database::new();
            font_db.load_system_fonts();

            let cjk_font_names = [
                "Noto Sans CJK SC",
                "Noto Sans CJK",
                "Microsoft YaHei",
                "微软雅黑",
            ];

            for font_name in &cjk_font_names {
                if let Some(face_id) = font_db.query(&fontdb::Query {
                    families: &[fontdb::Family::Name(font_name)],
                    weight: fontdb::Weight::NORMAL,
                    stretch: fontdb::Stretch::Normal,
                    style: fontdb::Style::Normal,
                }) {
                    if let Some(font_data) =
                        font_db.with_face_data(face_id, |data, _| Some(data.to_vec()))
                        && let Some(font_bytes) = font_data
                    {
                        return font_bytes;
                    }
                }
            }

            panic!("cannot find cjk font")
        }
    })
}

#[cfg(all(feature = "embedded_font", feature = "system_font"))]
compile_error!("Features 'embedded_font' and 'system_fonts' cannot be enabled together");
