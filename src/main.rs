use anyhow::{Context, Result};
use raylib::prelude::*;
use std::env;
use std::path::PathBuf;

mod app;
mod code_window;
mod constants;
mod helpers;
mod icons;
mod model;
mod sidebar;
mod theme;

use app::AppState;
use constants::*;
use theme::{ColorKind, Palette, default_palette, load_tmtheme_palette};

pub enum AppFont {
    Owned(Font),
    Default(WeakFont),
}

impl AppFont {
    fn owned(font: Font) -> Self {
        Self::Owned(font)
    }

    fn default_font(rl: &RaylibHandle) -> Self {
        Self::Default(rl.get_font_default())
    }

    fn draw_text_ex(
        &self,
        d: &mut RaylibDrawHandle,
        text: impl AsRef<str>,
        pos: Vector2,
        size: f32,
        spacing: f32,
        color: Color,
    ) {
        let t = text.as_ref();
        match self {
            AppFont::Owned(f) => d.draw_text_ex(f, t, pos, size, spacing, color),
            AppFont::Default(f) => d.draw_text_ex(f, t, pos, size, spacing, color),
        }
    }

    fn measure_width(&self, text: impl AsRef<str>, size: f32, spacing: f32) -> f32 {
        match self {
            AppFont::Owned(f) => f.measure_text(text.as_ref(), size, spacing).x,
            AppFont::Default(f) => f.measure_text(text.as_ref(), size, spacing).x,
        }
    }
}

fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);

    let project = model::ProjectModel::load(&root)
        .with_context(|| format!("loading project at {}", root.display()))?;
    let (mut rl, thread) = raylib::init()
        .size(1280, 780)
        .resizable()
        .title("Rust Trace Viewer")
        .msaa_4x()
        .build();
    rl.set_target_fps(60);
    let font = load_monospace_font(&mut rl, &thread);

    let palette = load_tmtheme_palette().unwrap_or_else(default_palette);
    let mut app = AppState::new(project, &mut rl, &thread, palette);

    while !rl.window_should_close() {
        let mouse = rl.get_mouse_position();
        let wheel = rl.get_mouse_wheel_move();
        let left_pressed = rl.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT);
        let left_down = rl.is_mouse_button_down(MouseButton::MOUSE_BUTTON_LEFT);
        let shift_down = rl.is_key_down(KeyboardKey::KEY_LEFT_SHIFT)
            || rl.is_key_down(KeyboardKey::KEY_RIGHT_SHIFT);

        let typed = collect_typed_chars(&mut rl);
        let backspace = rl.is_key_pressed(KeyboardKey::KEY_BACKSPACE);
        app.handle_input(
            &font,
            mouse,
            wheel,
            left_pressed,
            left_down,
            typed,
            backspace,
            shift_down,
            rl.get_screen_height() as f32,
        );

        let mut d = rl.begin_drawing(&thread);
        app.draw(&mut d, &font, mouse);
    }

    app.save_layout().context("saving layout")?;
    Ok(())
}

pub fn token_rect(
    font: &AppFont,
    line: &str,
    start: usize,
    len: usize,
    base_x: f32,
    y: f32,
) -> Rectangle {
    let before = &line[..start];
    let token = &line[start..start + len];
    let width_before = font.measure_width(before, FONT_SIZE, 0.0);
    let width_token = font.measure_width(token, FONT_SIZE, 0.0);
    Rectangle {
        x: base_x + width_before,
        y,
        width: width_token,
        height: LINE_HEIGHT,
    }
}

pub fn point_in_rect(point: Vector2, rect: Rectangle) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}

pub fn collect_typed_chars(rl: &mut RaylibHandle) -> String {
    let mut out = String::new();
    while let Some(ch) = rl.get_char_pressed() {
        if !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

pub fn load_monospace_font(rl: &mut RaylibHandle, thread: &RaylibThread) -> AppFont {
    let candidates = vec![
        env::var("TRACE_VIEWER_FONT").ok(),
        Some("data/fonts/MonaspaceNeon-Regular.otf".to_string()),
        Some("data/fonts/PressStart2P-Regular.ttf".to_string()),
        Some("C:\\Windows\\Fonts\\Consola.ttf".to_string()),
        Some("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf".to_string()),
        Some("/Library/Fonts/MesloLGS NF Regular.ttf".to_string()),
    ];

    for path in candidates.into_iter().flatten() {
        let p = PathBuf::from(&path);
        if !p.exists() {
            continue;
        }
        if let Some(path_str) = p.to_str() {
            if let Ok(font) = rl.load_font_ex(thread, path_str, FONT_SIZE as i32, None) {
                return AppFont::owned(font);
            }
        }
    }

    AppFont::default_font(rl)
}

pub fn draw_segments(
    d: &mut RaylibDrawHandle,
    font: &AppFont,
    base_x: f32,
    y: f32,
    segments: &[(String, ColorKind)],
    palette: &Palette,
) {
    let mut x = base_x;
    for (text, color) in segments {
        let c = match color {
            ColorKind::Text => palette.text,
            ColorKind::Comment => palette.comment,
            ColorKind::String => palette.string,
            ColorKind::Keyword => palette.keyword,
            ColorKind::Call => palette.call,
        };
        font.draw_text_ex(d, text, Vector2::new(x, y), FONT_SIZE, 0.0, c);
        x += font.measure_width(text, FONT_SIZE, 0.0);
    }
}

pub fn estimated_line_width(line: &str) -> f32 {
    line.chars().count() as f32 * FONT_SIZE * 0.6
}
