use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher};
use raylib::prelude::*;
use std::env;
use std::path::PathBuf;
use std::sync::mpsc::channel;

mod app;
mod code_window;
mod constants;
mod helpers;
mod icons;
mod model;
mod sidebar;
mod theme;

use app::{AppState, DepMode, ThemeMode};
use constants::*;
use theme::{default_palette, load_tmtheme_palette};

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
        let snapped = Vector2::new(pos.x.round(), pos.y.round());
        match self {
            AppFont::Owned(f) => d.draw_text_ex(f, t, snapped, size, spacing, color),
            AppFont::Default(f) => d.draw_text_ex(f, t, snapped, size, spacing, color),
        }
    }

    fn measure_width(&self, text: impl AsRef<str>, size: f32, spacing: f32) -> f32 {
        match self {
            AppFont::Owned(f) => f.measure_text(text.as_ref(), size, spacing).x,
            AppFont::Default(f) => f.measure_text(text.as_ref(), size, spacing).x,
        }
    }

    fn apply_filter(&self, thread: &RaylibThread) {
        let filter = TextureFilter::TEXTURE_FILTER_POINT;
        match self {
            AppFont::Owned(f) => {
                f.texture().set_texture_filter(thread, filter);
            }
            AppFont::Default(f) => {
                f.texture().set_texture_filter(thread, filter);
            }
        }
    }
}

fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);

    let dep_mode = DepMode::from_env();
    let initial_include_deps = dep_mode.initial_include_deps();
    let project = model::ProjectModel::load(&root, initial_include_deps)
        .with_context(|| format!("loading project at {}", root.display()))?;
    let (mut rl, thread) = raylib::init()
        .size(1280, 780)
        .resizable()
        .title("Code Trace Viewer")
        .msaa_4x()
        .build();
    rl.set_target_fps(60);
    let font = load_monospace_font(&mut rl, &thread);

    let app_palette = default_palette();
    let code_palette_opt = load_tmtheme_palette();
    let (code_palette, theme_mode) = match code_palette_opt {
        Some(pal) => (pal, ThemeMode::Code),
        None => (app_palette, ThemeMode::Application),
    };
    let mut app = AppState::new(
        project,
        &mut rl,
        &thread,
        app_palette,
        code_palette,
        theme_mode,
        dep_mode,
        initial_include_deps,
    );
    app.warm_load_deps();

    let (fs_tx, fs_rx) = channel();
    let mut _watcher = notify::recommended_watcher(move |res| {
        let _ = fs_tx.send(res);
    })?;
    _watcher.watch(&root, RecursiveMode::Recursive)?;

    while !rl.window_should_close() {
        while let Ok(event) = fs_rx.try_recv() {
            if let Ok(evt) = event {
                use notify::event::EventKind;
                match evt.kind {
                    EventKind::Modify(_)
                    | EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Any => {
                        app.mark_project_dirty();
                    }
                    _ => {}
                }
            } else {
                app.mark_project_dirty();
            }
        }
        app.reload_project_if_needed()?;

        let mouse = rl.get_mouse_position();
        let wheel = rl.get_mouse_wheel_move();
        let left_pressed = rl.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT);
        let left_down = rl.is_mouse_button_down(MouseButton::MOUSE_BUTTON_LEFT);
        let middle_pressed = rl.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_MIDDLE);
        let middle_down = rl.is_mouse_button_down(MouseButton::MOUSE_BUTTON_MIDDLE);
        let shift_down = rl.is_key_down(KeyboardKey::KEY_LEFT_SHIFT)
            || rl.is_key_down(KeyboardKey::KEY_RIGHT_SHIFT);
        let ctrl_down = rl.is_key_down(KeyboardKey::KEY_LEFT_CONTROL)
            || rl.is_key_down(KeyboardKey::KEY_RIGHT_CONTROL);
        let meta_down = rl.is_key_down(KeyboardKey::KEY_LEFT_SUPER)
            || rl.is_key_down(KeyboardKey::KEY_RIGHT_SUPER);
        let space_down = rl.is_key_down(KeyboardKey::KEY_SPACE);
        let meta_close_pressed = meta_down && rl.is_key_pressed(KeyboardKey::KEY_W);

        let typed = collect_typed_chars(&mut rl);
        let backspace = rl.is_key_pressed(KeyboardKey::KEY_BACKSPACE);
        app.handle_input(
            &font,
            mouse,
            wheel,
            left_pressed,
            left_down,
            middle_pressed,
            middle_down,
            typed,
            backspace,
            shift_down,
            ctrl_down,
            space_down,
            rl.get_screen_width() as f32,
            rl.get_screen_height() as f32,
            meta_close_pressed,
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
    let x = (base_x + width_before).round();
    Rectangle {
        x,
        y: y.round(),
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
                let app_font = AppFont::owned(font);
                app_font.apply_filter(thread);
                return app_font;
            }
        }
    }

    let font = AppFont::default_font(rl);
    font.apply_filter(thread);
    font
}

pub fn draw_segments(
    d: &mut RaylibDrawHandle,
    font: &AppFont,
    base_x: f32,
    y: f32,
    line: &str,
    segments: &[(std::ops::Range<usize>, Color)],
) {
    let mut x = base_x;
    for (range, color) in segments {
        let text = &line[range.clone()];
        font.draw_text_ex(d, text, Vector2::new(x, y), FONT_SIZE, 0.0, *color);
        x += font.measure_width(text, FONT_SIZE, 0.0);
    }
}

pub fn estimated_line_width(line: &str) -> f32 {
    line.chars().count() as f32 * FONT_SIZE * 0.6
}
