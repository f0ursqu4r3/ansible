use anyhow::{Context, Result};
use raylib::prelude::*;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const FONT_SIZE: f32 = 18.0;
const LINE_HEIGHT: f32 = FONT_SIZE * 1.4;
const TITLE_BAR_HEIGHT: f32 = 32.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const CODE_X_OFFSET: f32 = 52.0;
type AppFont = WeakFont;

#[derive(Clone, Debug)]
struct FunctionDef {
    name: String,
    line: usize,
    col: usize,
}

#[derive(Clone, Debug)]
struct FunctionCall {
    name: String,
    line: usize,
    col: usize,
    len: usize,
}

#[derive(Clone, Debug)]
struct ParsedFile {
    path: PathBuf,
    lines: Vec<String>,
    defs: Vec<FunctionDef>,
    calls: Vec<FunctionCall>,
}

impl ParsedFile {
    fn calls_on_line(&self, line: usize) -> impl Iterator<Item = &FunctionCall> {
        self.calls.iter().filter(move |c| c.line == line)
    }
}

#[derive(Clone, Debug)]
struct DefinitionLocation {
    file: PathBuf,
    line: usize,
    col: usize,
}

struct ProjectModel {
    root: PathBuf,
    files: Vec<PathBuf>,
    parsed: HashMap<PathBuf, ParsedFile>,
    defs: HashMap<String, Vec<DefinitionLocation>>,
}

impl ProjectModel {
    fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let mut files = Vec::new();
        for entry in WalkDir::new(&root) {
            let entry = entry?;
            if entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .map(|ext| ext == "rs")
                    .unwrap_or(false)
            {
                files.push(entry.into_path());
            }
        }
        files.sort();

        let mut parsed = HashMap::new();
        let mut defs: HashMap<String, Vec<DefinitionLocation>> = HashMap::new();
        for file in &files {
            let pf = parse_rust_file(file)?;
            for def in &pf.defs {
                defs.entry(def.name.clone())
                    .or_default()
                    .push(DefinitionLocation {
                        file: file.clone(),
                        line: def.line,
                        col: def.col,
                    });
            }
            parsed.insert(file.clone(), pf);
        }

        Ok(Self {
            root,
            files,
            parsed,
            defs,
        })
    }

    fn display_name(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    }
}

#[derive(Clone, Debug)]
struct CodeWindow {
    id: usize,
    file: PathBuf,
    title: String,
    position: Vector2,
    size: Vector2,
    scroll: f32,
    is_dragging: bool,
    drag_offset: Vector2,
}

impl CodeWindow {
    fn content_rect(&self) -> Rectangle {
        Rectangle {
            x: self.position.x,
            y: self.position.y + TITLE_BAR_HEIGHT,
            width: self.size.x,
            height: self.size.y - TITLE_BAR_HEIGHT,
        }
    }

    fn title_rect(&self) -> Rectangle {
        Rectangle {
            x: self.position.x,
            y: self.position.y,
            width: self.size.x,
            height: TITLE_BAR_HEIGHT,
        }
    }
}

enum WindowAction {
    None,
    Close,
    OpenDefinition(DefinitionLocation),
    StartDrag(Vector2),
}

struct AppState {
    project: ProjectModel,
    windows: Vec<CodeWindow>,
    next_window_id: usize,
}

impl AppState {
    fn new(project: ProjectModel) -> Self {
        let mut state = Self {
            project,
            windows: Vec::new(),
            next_window_id: 1,
        };
        if let Some(first) = state.project.files.first() {
            state.open_file(first.clone(), None);
        }
        state
    }

    fn open_file(&mut self, path: PathBuf, jump_to: Option<usize>) {
        if let Some(idx) = self.windows.iter().position(|w| w.file == path) {
            let mut win = self.windows.remove(idx);
            if let Some(line) = jump_to {
                win.scroll = (line as f32 * LINE_HEIGHT - 40.0).max(0.0);
            }
            self.windows.push(win);
            return;
        }

        let pos = Vector2::new(
            SIDEBAR_WIDTH + 24.0 + (self.windows.len() as f32 * 18.0),
            40.0 + (self.windows.len() as f32 * 18.0),
        );
        let mut scroll = 0.0;
        if let Some(line) = jump_to {
            scroll = (line as f32 * LINE_HEIGHT - 40.0).max(0.0);
        }
        let win = CodeWindow {
            id: self.next_window_id,
            file: path.clone(),
            title: path.file_name().and_then(|s| s.to_str()).unwrap_or("file").to_string(),
            position: pos,
            size: Vector2::new(720.0, 460.0),
            scroll,
            is_dragging: false,
            drag_offset: Vector2 { x: 0.0, y: 0.0 },
        };
        self.next_window_id += 1;
        self.windows.push(win);
    }

    fn bring_to_front(&mut self, idx: usize) {
        if idx + 1 == self.windows.len() {
            return;
        }
        let w = self.windows.remove(idx);
        self.windows.push(w);
    }

    fn handle_input(
        &mut self,
        font: &AppFont,
        mouse: Vector2,
        wheel: f32,
        left_pressed: bool,
        left_down: bool,
    ) {
        if !left_down {
            for w in &mut self.windows {
                w.is_dragging = false;
            }
        }

        // Scroll when hovering over a code window content.
        if wheel.abs() > f32::EPSILON {
            for idx in (0..self.windows.len()).rev() {
                let content = self.windows[idx].content_rect();
                if point_in_rect(mouse, content) {
                    let max_scroll = self.max_scroll(idx);
                    let win = &mut self.windows[idx];
                    win.scroll = (win.scroll - wheel * LINE_HEIGHT).clamp(0.0, max_scroll);
                    break;
                }
            }
        }

        // Dragging updates.
        if left_down {
            for w in &mut self.windows {
                if w.is_dragging {
                    w.position = Vector2::new(mouse.x - w.drag_offset.x, mouse.y - w.drag_offset.y);
                }
            }
        }

        // Click handling: topmost window under cursor gets the event.
        if left_pressed {
            let mut handled = false;
            for idx in (0..self.windows.len()).rev() {
                if self.window_hit_test(idx, mouse) {
                    self.bring_to_front(idx);
                    let top_idx = self.windows.len() - 1;
                    let action = self.handle_window_click(font, top_idx, mouse);
                    match action {
                        WindowAction::Close => {
                            self.windows.pop();
                        }
                        WindowAction::OpenDefinition(def) => {
                            self.open_file(def.file.clone(), Some(def.line));
                        }
                        WindowAction::StartDrag(offset) => {
                            if let Some(win) = self.windows.last_mut() {
                                win.is_dragging = true;
                                win.drag_offset = offset;
                            }
                        }
                        WindowAction::None => {}
                    }
                    handled = true;
                    break;
                }
            }

            if !handled {
                self.handle_sidebar_click(mouse);
            }
        }
    }

    fn handle_sidebar_click(&mut self, mouse: Vector2) {
        if mouse.x > SIDEBAR_WIDTH {
            return;
        }
        let mut y = 12.0;
        for path in &self.project.files {
            let rect = Rectangle {
                x: 10.0,
                y,
                width: SIDEBAR_WIDTH - 20.0,
                height: 22.0,
            };
            if point_in_rect(mouse, rect) {
                self.open_file(path.clone(), None);
                break;
            }
            y += 24.0;
        }
    }

    fn handle_window_click(
        &mut self,
        font: &AppFont,
        idx: usize,
        mouse: Vector2,
    ) -> WindowAction {
        let win = &self.windows[idx];
        let title_rect = win.title_rect();
        let close_rect = Rectangle {
            x: title_rect.x + title_rect.width - 24.0,
            y: title_rect.y + 6.0,
            width: 16.0,
            height: 16.0,
        };

        if point_in_rect(mouse, close_rect) {
            return WindowAction::Close;
        }

        if point_in_rect(mouse, title_rect) {
            return WindowAction::StartDrag(Vector2 {
                x: mouse.x - win.position.x,
                y: mouse.y - win.position.y,
            });
        }

        let content_rect = win.content_rect();
        if !point_in_rect(mouse, content_rect) {
            return WindowAction::None;
        }

        if let Some(pf) = self.project.parsed.get(&win.file) {
            if let Some(def) = self.hit_test_calls(font, pf, win, mouse) {
                return WindowAction::OpenDefinition(def);
            }
        }

        WindowAction::None
    }

    fn hit_test_calls(
        &self,
        font: &AppFont,
        file: &ParsedFile,
        win: &CodeWindow,
        mouse: Vector2,
    ) -> Option<DefinitionLocation> {
        let content_rect = win.content_rect();
        let content_top = content_rect.y;
        let local_y = mouse.y - content_top + win.scroll;
        if local_y < 0.0 {
            return None;
        }
        let line_idx = (local_y / LINE_HEIGHT).floor() as usize;
        if line_idx >= file.lines.len() {
            return None;
        }
        let line = &file.lines[line_idx];
        let calls: Vec<&FunctionCall> = file.calls_on_line(line_idx).collect();
        if calls.is_empty() {
            return None;
        }

        for call in calls {
            let rect = token_rect(
                font,
                line,
                call.col,
                call.len,
                content_rect.x + CODE_X_OFFSET,
                content_top + (line_idx as f32 * LINE_HEIGHT) - win.scroll,
            );
            if point_in_rect(mouse, rect) {
                if let Some(defs) = self.project.defs.get(&call.name) {
                    return defs.first().cloned();
                }
            }
        }

        None
    }

    fn window_hit_test(&self, idx: usize, mouse: Vector2) -> bool {
        let win = &self.windows[idx];
        let rect = Rectangle {
            x: win.position.x,
            y: win.position.y,
            width: win.size.x,
            height: win.size.y,
        };
        point_in_rect(mouse, rect)
    }

    fn max_scroll(&self, idx: usize) -> f32 {
        if let Some(pf) = self.project.parsed.get(&self.windows[idx].file) {
            let total_height = pf.lines.len() as f32 * LINE_HEIGHT;
            let visible = self.windows[idx].size.y - TITLE_BAR_HEIGHT - 8.0;
            return (total_height - visible).max(0.0);
        }
        0.0
    }

    fn draw(&mut self, d: &mut RaylibDrawHandle, font: &AppFont) {
        d.clear_background(Color::new(18, 18, 24, 255));
        self.draw_sidebar(d, font);
        for idx in 0..self.windows.len() {
            let is_top = idx + 1 == self.windows.len();
            self.draw_window(d, font, idx, is_top);
        }
    }

    fn draw_sidebar(&self, d: &mut RaylibDrawHandle, font: &AppFont) {
        d.draw_rectangle(0, 0, SIDEBAR_WIDTH as i32, d.get_screen_height(), Color::new(28, 28, 36, 255));
        d.draw_text_ex(
            font,
            "Project",
            Vector2::new(12.0, 10.0),
            FONT_SIZE + 2.0,
            0.0,
            Color::new(200, 210, 255, 255),
        );

        let mut y = 40.0;
        for path in &self.project.files {
            let display = self.project.display_name(path);
            d.draw_text_ex(
                font,
                &display,
                Vector2::new(14.0, y),
                FONT_SIZE - 2.0,
                0.0,
                Color::new(210, 210, 220, 255),
            );
            y += 24.0;
        }
    }

    fn draw_window(&self, d: &mut RaylibDrawHandle, font: &AppFont, idx: usize, is_top: bool) {
        let win = &self.windows[idx];
        let bg = if is_top {
            Color::new(40, 42, 58, 240)
        } else {
            Color::new(30, 30, 40, 220)
        };
        d.draw_rectangle(
            win.position.x as i32,
            win.position.y as i32,
            win.size.x as i32,
            win.size.y as i32,
            bg,
        );

        let title_rect = win.title_rect();
        d.draw_rectangle(
            title_rect.x as i32,
            title_rect.y as i32,
            title_rect.width as i32,
            title_rect.height as i32,
            Color::new(60, 64, 90, 255),
        );
        d.draw_text_ex(
            font,
            &win.title,
            Vector2::new(title_rect.x + 8.0, title_rect.y + 8.0),
            FONT_SIZE,
            0.0,
            Color::new(230, 230, 240, 255),
        );
        let close_rect = Rectangle {
            x: title_rect.x + title_rect.width - 24.0,
            y: title_rect.y + 6.0,
            width: 16.0,
            height: 16.0,
        };
        d.draw_rectangle_lines(
            close_rect.x as i32,
            close_rect.y as i32,
            close_rect.width as i32,
            close_rect.height as i32,
            Color::new(230, 120, 120, 255),
        );
        d.draw_text_ex(
            font,
            "x",
            Vector2::new(close_rect.x + 3.0, close_rect.y - 1.0),
            FONT_SIZE,
            0.0,
            Color::new(230, 120, 120, 255),
        );

        if let Some(pf) = self.project.parsed.get(&win.file) {
            self.draw_code(d, font, pf, win);
        }
    }

    fn draw_code(&self, d: &mut RaylibDrawHandle, font: &AppFont, file: &ParsedFile, win: &CodeWindow) {
        let content_rect = win.content_rect();
        let top_visible = (win.scroll / LINE_HEIGHT).floor() as usize;
        let lines_visible = ((content_rect.height + LINE_HEIGHT) / LINE_HEIGHT).ceil() as usize;
        let bottom = (top_visible + lines_visible + 1).min(file.lines.len());
        let mut y = content_rect.y - (win.scroll % LINE_HEIGHT);

        for idx in top_visible..bottom {
            let line = &file.lines[idx];
            let text_start_x = content_rect.x + CODE_X_OFFSET;
            let line_num_color = Color::new(90, 100, 130, 255);
            d.draw_text_ex(
                font,
                &format!("{:>4}", idx + 1),
                Vector2::new(content_rect.x + 4.0, y),
                FONT_SIZE - 2.0,
                0.0,
                line_num_color,
            );

            let calls: Vec<&FunctionCall> = file.calls_on_line(idx).collect();
            if calls.is_empty() {
                d.draw_text_ex(
                    font,
                    line,
                    Vector2::new(text_start_x, y),
                    FONT_SIZE,
                    0.0,
                    Color::new(220, 220, 230, 255),
                );
            } else {
                self.draw_line_with_calls(d, font, line, text_start_x, y, &calls);
            }

            y += LINE_HEIGHT;
        }
    }

    fn draw_line_with_calls(
        &self,
        d: &mut RaylibDrawHandle,
        font: &AppFont,
        line: &str,
        base_x: f32,
        y: f32,
        calls: &[&FunctionCall],
    ) {
        let mut cursor = 0;
        let mut x = base_x;
        for call in calls {
            if call.col > cursor {
                let slice = &line[cursor..call.col];
                if !slice.is_empty() {
                    d.draw_text_ex(font, slice, Vector2::new(x, y), FONT_SIZE, 0.0, Color::new(220, 220, 230, 255));
                    x += font.measure_text(slice, FONT_SIZE, 0.0).x;
                }
            }
            let name = &line[call.col..call.col + call.len];
            d.draw_text_ex(font, name, Vector2::new(x, y), FONT_SIZE, 0.0, Color::new(120, 200, 255, 255));
            x += font.measure_text(name, FONT_SIZE, 0.0).x;
            cursor = call.col + call.len;
        }
        if cursor < line.len() {
            let slice = &line[cursor..];
            d.draw_text_ex(font, slice, Vector2::new(x, y), FONT_SIZE, 0.0, Color::new(220, 220, 230, 255));
        }
    }
}

fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);

    let project = ProjectModel::load(&root).with_context(|| format!("loading project at {}", root.display()))?;
    let (mut rl, thread) = raylib::init()
        .size(1280, 780)
        .title("Rust Trace Viewer")
        .msaa_4x()
        .build();
    rl.set_target_fps(60);
    let font = rl.get_font_default();

    let mut app = AppState::new(project);

    while !rl.window_should_close() {
        let mouse = rl.get_mouse_position();
        let wheel = rl.get_mouse_wheel_move();
        let left_pressed = rl.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT);
        let left_down = rl.is_mouse_button_down(MouseButton::MOUSE_BUTTON_LEFT);

        app.handle_input(&font, mouse, wheel, left_pressed, left_down);

        let mut d = rl.begin_drawing(&thread);
        app.draw(&mut d, &font);
    }

    Ok(())
}

fn parse_rust_file(path: &Path) -> Result<ParsedFile> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

    let fn_re = Regex::new(r"(?m)^\s*(pub\s+)?(async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let call_re = Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap();

    let mut defs = Vec::new();
    let mut calls = Vec::new();

    for cap in fn_re.captures_iter(&content) {
        if let Some(m) = cap.get(3) {
            let (line, col) = offset_to_line_col(&content, m.start());
            defs.push(FunctionDef {
                name: m.as_str().to_string(),
                line,
                col,
            });
        }
    }

    let keywords: HashSet<&'static str> = [
        "if", "else", "for", "while", "loop", "match", "fn", "pub", "impl", "struct", "enum",
        "use", "let", "in", "where", "return",
    ]
    .into_iter()
    .collect();

    for (line_idx, line) in lines.iter().enumerate() {
        for cap in call_re.captures_iter(line) {
            if let Some(m) = cap.get(1) {
                let name = m.as_str();
                if keywords.contains(name) {
                    continue;
                }
                calls.push(FunctionCall {
                    name: name.to_string(),
                    line: line_idx,
                    col: m.start(),
                    len: m.as_str().len(),
                });
            }
        }
    }

    Ok(ParsedFile {
        path: path.to_path_buf(),
        lines,
        defs,
        calls,
    })
}

fn offset_to_line_col(content: &str, offset: usize) -> (usize, usize) {
    let mut line = 0;
    let mut col = 0;
    let mut count = 0;
    for ch in content.chars() {
        if count == offset {
            break;
        }
        count += ch.len_utf8();
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn token_rect(
    font: &AppFont,
    line: &str,
    start: usize,
    len: usize,
    base_x: f32,
    y: f32,
) -> Rectangle {
    let before = &line[..start];
    let token = &line[start..start + len];
    let width_before = font.measure_text(before, FONT_SIZE, 0.0).x;
    let width_token = font.measure_text(token, FONT_SIZE, 0.0).x;
    Rectangle {
        x: base_x + width_before,
        y,
        width: width_token,
        height: LINE_HEIGHT,
    }
}

fn point_in_rect(point: Vector2, rect: Rectangle) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}
