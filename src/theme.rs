use std::env;
use std::fs;
use std::path::PathBuf;

use plist::Value as PlistValue;
use raylib::prelude::Color;

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color,
    pub sidebar: Color,
    pub window_top: Color,
    pub window: Color,
    pub title: Color,
    pub text: Color,
    pub comment: Color,
    pub string: Color,
    pub keyword: Color,
    pub call: Color,
    pub line_num: Color,
    pub close: Color,
    pub project: Color,
    pub sidebar_text: Color,
    pub sidebar_highlight: Color,
    pub search_bg: Color,
    pub breadcrumb: Color,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ColorKind {
    Text,
    Comment,
    String,
    Keyword,
    Call,
}

pub fn default_palette() -> Palette {
    Palette {
        bg: Color::new(18, 18, 24, 255),
        sidebar: Color::new(28, 28, 36, 255),
        window_top: Color::new(40, 42, 58, 240),
        window: Color::new(30, 30, 40, 220),
        title: Color::new(60, 64, 90, 255),
        text: Color::new(220, 220, 230, 255),
        comment: Color::new(120, 130, 150, 255),
        string: Color::new(180, 200, 140, 255),
        keyword: Color::new(255, 170, 120, 255),
        call: Color::new(120, 200, 255, 255),
        line_num: Color::new(90, 100, 130, 255),
        close: Color::new(230, 120, 120, 255),
        project: Color::new(200, 210, 255, 255),
        sidebar_text: Color::new(210, 210, 220, 255),
        sidebar_highlight: Color::new(70, 90, 140, 140),
        search_bg: Color::new(40, 44, 60, 255),
        breadcrumb: Color::new(140, 150, 170, 255),
    }
}

pub fn load_tmtheme_palette() -> Option<Palette> {
    let theme_path = env::var("TM_THEME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            let p = PathBuf::from("data/theme.tmtheme");
            if p.exists() {
                Some(p)
            } else {
                None
            }
        })?;
    let data = fs::read(theme_path).ok()?;
    let plist = PlistValue::from_reader_xml(&*data).ok()?;
    let plist_dict = plist.as_dictionary()?;
    let settings = plist_dict.get("settings")?.as_array()?;

    let mut palette = default_palette();

    for item in settings {
        let dict = item.as_dictionary()?;
        let setting_values = dict.get("settings")?.as_dictionary()?;
        let scope = dict
            .get("scope")
            .and_then(|v| v.as_string())
            .unwrap_or("");
        if scope.is_empty() {
            if let Some(bg) = setting_values
                .get("background")
                .and_then(|v| v.as_string())
                .and_then(parse_hex_color)
            {
                palette.bg = bg;
                palette.sidebar = bg;
                palette.window = bg;
            }
            if let Some(fg) = setting_values
                .get("foreground")
                .and_then(|v| v.as_string())
                .and_then(parse_hex_color)
            {
                palette.text = fg;
                palette.project = fg;
                palette.sidebar_text = fg;
                palette.line_num = fg;
                palette.breadcrumb = fg;
            }
            continue;
        }

        let fg = setting_values
            .get("foreground")
            .and_then(|v| v.as_string())
            .and_then(parse_hex_color);
        if let Some(color) = fg {
            if scope.contains("comment") {
                palette.comment = color;
            }
            if scope.contains("string") {
                palette.string = color;
            }
            if scope.contains("keyword") {
                palette.keyword = color;
            }
            if scope.contains("entity.name.function") || scope.contains("support.function") {
                palette.call = color;
            }
        }
    }

    palette.sidebar_highlight = Color::new(
        (palette.project.r as f32 * 0.3 + palette.bg.r as f32 * 0.7) as u8,
        (palette.project.g as f32 * 0.3 + palette.bg.g as f32 * 0.7) as u8,
        (palette.project.b as f32 * 0.3 + palette.bg.b as f32 * 0.7) as u8,
        140,
    );
    palette.search_bg = Color::new(
        (palette.bg.r as f32 * 0.9) as u8,
        (palette.bg.g as f32 * 0.9) as u8,
        (palette.bg.b as f32 * 0.9) as u8,
        255,
    );

    Some(palette)
}

fn parse_hex_color(s: &str) -> Option<Color> {
    let hex = s.trim().trim_start_matches('#');
    let bytes = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => {
            let r = ((bytes >> 16) & 0xFF) as u8;
            let g = ((bytes >> 8) & 0xFF) as u8;
            let b = (bytes & 0xFF) as u8;
            Some(Color::new(r, g, b, 255))
        }
        8 => {
            let a = ((bytes >> 24) & 0xFF) as u8;
            let r = ((bytes >> 16) & 0xFF) as u8;
            let g = ((bytes >> 8) & 0xFF) as u8;
            let b = (bytes & 0xFF) as u8;
            Some(Color::new(r, g, b, a))
        }
        _ => None,
    }
}
