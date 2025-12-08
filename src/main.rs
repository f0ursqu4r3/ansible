use anyhow::{Context, Result};
use proc_macro2::Span;
use raylib::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use walkdir::WalkDir;

const FONT_SIZE: f32 = 10.0;
const LINE_HEIGHT: f32 = FONT_SIZE * 1.4;
const TITLE_BAR_HEIGHT: f32 = 32.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const CODE_X_OFFSET: f32 = 52.0;
const BREADCRUMB_HEIGHT: f32 = 20.0;
const LAYOUT_FILE: &str = ".trace_viewer_layout.json";

const COLOR_BG: Color = Color::new(18, 18, 24, 255);
const COLOR_SIDEBAR: Color = Color::new(28, 28, 36, 255);
const COLOR_WINDOW_TOP: Color = Color::new(40, 42, 58, 240);
const COLOR_WINDOW: Color = Color::new(30, 30, 40, 220);
const COLOR_TITLE: Color = Color::new(60, 64, 90, 255);
const COLOR_TEXT: Color = Color::new(220, 220, 230, 255);
const COLOR_COMMENT: Color = Color::new(120, 130, 150, 255);
const COLOR_STRING: Color = Color::new(180, 200, 140, 255);
const COLOR_KEYWORD: Color = Color::new(255, 170, 120, 255);
const COLOR_CALL: Color = Color::new(120, 200, 255, 255);
const COLOR_LINE_NUM: Color = Color::new(90, 100, 130, 255);
const COLOR_CLOSE: Color = Color::new(230, 120, 120, 255);
const COLOR_PROJECT: Color = Color::new(200, 210, 255, 255);
const COLOR_SIDEBAR_TEXT: Color = Color::new(210, 210, 220, 255);
const COLOR_SIDEBAR_HIGHLIGHT: Color = Color::new(70, 90, 140, 140);
const COLOR_SEARCH_BG: Color = Color::new(40, 44, 60, 255);
const COLOR_BREADCRUMB: Color = Color::new(140, 150, 170, 255);
const KEYWORDS: [&str; 21] = [
    "if", "else", "for", "while", "loop", "match", "fn", "pub", "impl", "struct", "enum", "use",
    "let", "in", "where", "return", "async", "mod", "trait", "const", "static",
];

enum AppFont {
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

#[derive(Clone, Debug)]
struct FunctionDef {
    name: String,
    module_path: String,
    line: usize,
    col: usize,
}

#[derive(Clone, Debug)]
struct FunctionCall {
    name: String,
    module_path: String,
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
    module_path: String,
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
                        module_path: def.module_path.clone(),
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
    scroll_x: f32,
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

#[derive(Serialize, Deserialize)]
struct SavedWindow {
    file: String,
    position: (f32, f32),
    size: (f32, f32),
    scroll: f32,
    #[serde(default)]
    scroll_x: f32,
}

#[derive(Serialize, Deserialize)]
struct SavedLayout {
    windows: Vec<SavedWindow>,
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
    search_query: String,
    search_focused: bool,
}

impl AppState {
    fn layout_path(&self) -> PathBuf {
        self.project.root.join(LAYOUT_FILE)
    }

    fn load_layout(&mut self) {
        let path = self.layout_path();
        let Ok(text) = fs::read_to_string(&path) else {
            return;
        };
        if let Ok(layout) = serde_json::from_str::<SavedLayout>(&text) {
            for saved in layout.windows {
                let file = self.project.root.join(&saved.file);
                if !self.project.parsed.contains_key(&file) {
                    continue;
                }
                let title = file
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("file")
                    .to_string();
                let mut win = CodeWindow {
                    id: self.next_window_id,
                    file,
                    title,
                    position: Vector2::new(saved.position.0, saved.position.1),
                    size: Vector2::new(saved.size.0, saved.size.1),
                    scroll: saved.scroll,
                    scroll_x: saved.scroll_x,
                    is_dragging: false,
                    drag_offset: Vector2 { x: 0.0, y: 0.0 },
                };
                if let Some(max) = self.parsed_height(&win.file) {
                    win.scroll = win.scroll.min(max);
                }
                if let Some(max_x) = self.max_scroll_x(&win) {
                    win.scroll_x = win.scroll_x.min(max_x);
                }
                self.windows.push(win);
                self.next_window_id += 1;
            }
        }
    }

    fn save_layout(&self) -> Result<()> {
        let windows: Vec<SavedWindow> = self
            .windows
            .iter()
            .map(|w| SavedWindow {
                file: self.project.display_name(&w.file),
                position: (w.position.x, w.position.y),
                size: (w.size.x, w.size.y),
                scroll: w.scroll,
                scroll_x: w.scroll_x,
            })
            .collect();
        let layout = SavedLayout { windows };
        let path = self.layout_path();
        let text = serde_json::to_string_pretty(&layout)?;
        fs::write(path, text)?;
        Ok(())
    }

    fn new(project: ProjectModel) -> Self {
        let mut state = Self {
            project,
            windows: Vec::new(),
            next_window_id: 1,
            search_query: String::new(),
            search_focused: false,
        };
        state.load_layout();
        if state.windows.is_empty() {
            if let Some(first) = state.project.files.first() {
                state.open_file(first.clone(), None);
            }
        }
        state
    }

    fn open_file(&mut self, path: PathBuf, jump_to: Option<usize>) {
        if let Some(idx) = self.windows.iter().position(|w| w.file == path) {
            let mut win = self.windows.remove(idx);
            if let Some(line) = jump_to {
                win.scroll = (line as f32 * LINE_HEIGHT - 40.0).max(0.0);
            }
            if let Some(max) = self.parsed_height(&win.file) {
                win.scroll = win.scroll.min(max);
            }
            if let Some(max_x) = self.max_scroll_x(&win) {
                win.scroll_x = win.scroll_x.min(max_x);
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
        let mut win = CodeWindow {
            id: self.next_window_id,
            file: path.clone(),
            title: path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file")
                .to_string(),
            position: pos,
            size: Vector2::new(720.0, 460.0),
            scroll,
            scroll_x: 0.0,
            is_dragging: false,
            drag_offset: Vector2 { x: 0.0, y: 0.0 },
        };
        if let Some(max) = self.parsed_height(&win.file) {
            win.scroll = win.scroll.min(max);
        }
        if let Some(max_x) = self.max_scroll_x(&win) {
            win.scroll_x = win.scroll_x.min(max_x);
        }
        self.next_window_id += 1;
        self.windows.push(win);
    }

    fn parsed_height(&self, file: &Path) -> Option<f32> {
        self.project.parsed.get(file).map(|pf| {
            let total_height = pf.lines.len() as f32 * LINE_HEIGHT;
            (total_height - (TITLE_BAR_HEIGHT + BREADCRUMB_HEIGHT + 8.0)).max(0.0)
        })
    }

    fn max_scroll_x(&self, win: &CodeWindow) -> Option<f32> {
        let pf = self.project.parsed.get(&win.file)?;
        let mut max_width = 0.0f32;
        for line in &pf.lines {
            let w = estimated_line_width(line);
            if w > max_width {
                max_width = w;
            }
        }
        let visible = (win.size.x - CODE_X_OFFSET - 24.0).max(32.0);
        Some((max_width - visible).max(0.0))
    }

    fn bring_to_front(&mut self, idx: usize) {
        if idx + 1 == self.windows.len() {
            return;
        }
        let w = self.windows.remove(idx);
        self.windows.push(w);
    }

    fn search_rect(&self) -> Rectangle {
        Rectangle {
            x: 12.0,
            y: 40.0,
            width: SIDEBAR_WIDTH - 24.0,
            height: 26.0,
        }
    }

    fn handle_input(
        &mut self,
        font: &AppFont,
        mouse: Vector2,
        wheel: f32,
        left_pressed: bool,
        left_down: bool,
        typed: String,
        backspace: bool,
        shift_down: bool,
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
                    if shift_down {
                        let max_x = {
                            let win_ref = &self.windows[idx];
                            self.max_scroll_x(win_ref)
                        };
                        let win = &mut self.windows[idx];
                        if let Some(max_x) = max_x {
                            win.scroll_x = (win.scroll_x - wheel * LINE_HEIGHT).clamp(0.0, max_x);
                        }
                    } else {
                        let max_scroll = self.max_scroll(idx);
                        let win = &mut self.windows[idx];
                        win.scroll = (win.scroll - wheel * LINE_HEIGHT).clamp(0.0, max_scroll);
                    }
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
                handled = self.handle_sidebar_click(mouse);
            }

            if !handled && point_in_rect(mouse, self.search_rect()) {
                self.search_focused = true;
            } else if !handled {
                self.search_focused = false;
            }
        }

        if self.search_focused {
            if backspace {
                self.search_query.pop();
            }
            if !typed.is_empty() {
                self.search_query.push_str(&typed);
            }
        }
    }

    fn handle_sidebar_click(&mut self, mouse: Vector2) -> bool {
        if mouse.x > SIDEBAR_WIDTH {
            return false;
        }
        let query = self.search_query.to_lowercase();
        let search_rect = self.search_rect();
        let mut y = search_rect.y + search_rect.height + 10.0;
        let files: Vec<_> = if query.is_empty() {
            self.project.files.iter().collect()
        } else {
            self.project
                .files
                .iter()
                .filter(|p| self.project.display_name(p).to_lowercase().contains(&query))
                .collect()
        };
        for path in files {
            let rect = Rectangle {
                x: 10.0,
                y,
                width: SIDEBAR_WIDTH - 20.0,
                height: 22.0,
            };
            if point_in_rect(mouse, rect) {
                self.open_file(path.clone(), None);
                return true;
            }
            y += 24.0;
        }

        if !query.is_empty() {
            y += 8.0 + 18.0; // spacing + "Matches" header
            for (_, def) in self
                .project
                .defs
                .iter()
                .filter(|(name, _)| name.to_lowercase().contains(&query))
                .take(15)
            {
                let rect = Rectangle {
                    x: 10.0,
                    y,
                    width: SIDEBAR_WIDTH - 20.0,
                    height: 20.0,
                };
                if point_in_rect(mouse, rect) {
                    let target = def.first().unwrap();
                    self.open_file(target.file.clone(), Some(target.line));
                    return true;
                }
                y += 20.0;
            }
        }
        false
    }

    fn handle_window_click(&mut self, font: &AppFont, idx: usize, mouse: Vector2) -> WindowAction {
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
        let content_top = content_rect.y + BREADCRUMB_HEIGHT;
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
                content_rect.x + CODE_X_OFFSET - win.scroll_x,
                content_top + (line_idx as f32 * LINE_HEIGHT) - win.scroll,
            );
            if point_in_rect(mouse, rect) {
                if let Some(defs) = self.project.defs.get(&call.name) {
                    if let Some(exact) = defs.iter().find(|d| d.module_path == call.module_path) {
                        return Some(exact.clone());
                    }
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
            let visible = self.windows[idx].size.y - TITLE_BAR_HEIGHT - BREADCRUMB_HEIGHT - 8.0;
            return (total_height - visible).max(0.0);
        }
        0.0
    }

    fn draw(&mut self, d: &mut RaylibDrawHandle, font: &AppFont, mouse: Vector2) {
        d.clear_background(COLOR_BG);
        self.draw_sidebar(d, font, mouse);
        for idx in 0..self.windows.len() {
            let is_top = idx + 1 == self.windows.len();
            self.draw_window(d, font, idx, is_top);
        }
    }

    fn draw_sidebar(&self, d: &mut RaylibDrawHandle, font: &AppFont, mouse: Vector2) {
        d.draw_rectangle(
            0,
            0,
            SIDEBAR_WIDTH as i32,
            d.get_screen_height(),
            COLOR_SIDEBAR,
        );
        font.draw_text_ex(
            d,
            "Project",
            Vector2::new(12.0, 10.0),
            FONT_SIZE + 2.0,
            0.0,
            COLOR_PROJECT,
        );

        let search_rect = self.search_rect();
        d.draw_rectangle(
            search_rect.x as i32,
            search_rect.y as i32,
            search_rect.width as i32,
            search_rect.height as i32,
            COLOR_SEARCH_BG,
        );
        if self.search_focused {
            d.draw_rectangle_lines(
                (search_rect.x - 1.0) as i32,
                (search_rect.y - 1.0) as i32,
                (search_rect.width + 2.0) as i32,
                (search_rect.height + 2.0) as i32,
                COLOR_PROJECT,
            );
        }
        let search_text = if self.search_query.is_empty() {
            "search..."
        } else {
            &self.search_query
        };
        let search_color = if self.search_query.is_empty() {
            COLOR_LINE_NUM
        } else {
            COLOR_SIDEBAR_TEXT
        };
        font.draw_text_ex(
            d,
            search_text,
            Vector2::new(search_rect.x + 6.0, search_rect.y + 4.0),
            FONT_SIZE - 2.0,
            0.0,
            search_color,
        );

        let query = self.search_query.to_lowercase();
        let mut y = search_rect.y + search_rect.height + 10.0;
        let files: Vec<_> = if query.is_empty() {
            self.project.files.iter().collect()
        } else {
            self.project
                .files
                .iter()
                .filter(|p| self.project.display_name(p).to_lowercase().contains(&query))
                .collect()
        };

        for path in files {
            let display = self.project.display_name(path);
            let rect = Rectangle {
                x: 10.0,
                y,
                width: SIDEBAR_WIDTH - 20.0,
                height: 22.0,
            };
            if point_in_rect(mouse, rect) {
                d.draw_rectangle(
                    rect.x as i32,
                    rect.y as i32,
                    rect.width as i32,
                    rect.height as i32,
                    COLOR_SIDEBAR_HIGHLIGHT,
                );
            }
            font.draw_text_ex(
                d,
                &display,
                Vector2::new(rect.x + 4.0, y),
                FONT_SIZE - 2.0,
                0.0,
                COLOR_SIDEBAR_TEXT,
            );
            y += 24.0;
        }

        if !query.is_empty() {
            y += 8.0;
            font.draw_text_ex(
                d,
                "Matches",
                Vector2::new(12.0, y),
                FONT_SIZE - 2.0,
                0.0,
                COLOR_PROJECT,
            );
            y += 18.0;
            for (def_name, def) in self
                .project
                .defs
                .iter()
                .filter(|(name, _)| name.to_lowercase().contains(&query))
                .take(15)
            {
                let target = def.first().unwrap();
                let rect = Rectangle {
                    x: 10.0,
                    y,
                    width: SIDEBAR_WIDTH - 20.0,
                    height: 20.0,
                };
                if point_in_rect(mouse, rect) {
                    d.draw_rectangle(
                        rect.x as i32,
                        rect.y as i32,
                        rect.width as i32,
                        rect.height as i32,
                        COLOR_SIDEBAR_HIGHLIGHT,
                    );
                }
                let label = format!(
                    "{} ({})",
                    def_name,
                    target
                        .module_path
                        .strip_prefix("crate::")
                        .unwrap_or(&target.module_path)
                );
                font.draw_text_ex(
                    d,
                    &label,
                    Vector2::new(rect.x + 4.0, y),
                    FONT_SIZE - 4.0,
                    0.0,
                    COLOR_SIDEBAR_TEXT,
                );
                y += 20.0;
            }
        }
    }

    fn draw_window(&self, d: &mut RaylibDrawHandle, font: &AppFont, idx: usize, is_top: bool) {
        let win = &self.windows[idx];
        let bg = if is_top {
            COLOR_WINDOW_TOP
        } else {
            COLOR_WINDOW
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
            COLOR_TITLE,
        );
        font.draw_text_ex(
            d,
            &win.title,
            Vector2::new(title_rect.x + 8.0, title_rect.y + 8.0),
            FONT_SIZE,
            0.0,
            COLOR_TEXT,
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
            COLOR_CLOSE,
        );
        font.draw_text_ex(
            d,
            "x",
            Vector2::new(close_rect.x + 3.0, close_rect.y - 1.0),
            FONT_SIZE,
            0.0,
            COLOR_CLOSE,
        );

        if let Some(pf) = self.project.parsed.get(&win.file) {
            self.draw_code(d, font, pf, win);
        }
    }

    fn draw_code(
        &self,
        d: &mut RaylibDrawHandle,
        font: &AppFont,
        file: &ParsedFile,
        win: &CodeWindow,
    ) {
        let content_rect = win.content_rect();
        let mut breadcrumb = self.project.display_name(&file.path);
        if let Some(mod_path) = file.defs.first().map(|d| d.module_path.as_str()) {
            breadcrumb.push_str(" - ");
            breadcrumb.push_str(mod_path);
        }
        font.draw_text_ex(
            d,
            &breadcrumb,
            Vector2::new(content_rect.x + 8.0, content_rect.y + 2.0),
            FONT_SIZE - 2.0,
            0.0,
            COLOR_BREADCRUMB,
        );

        let start_y = content_rect.y + BREADCRUMB_HEIGHT;
        let top_visible = (win.scroll / LINE_HEIGHT).floor() as usize;
        let lines_visible =
            ((content_rect.height - BREADCRUMB_HEIGHT + LINE_HEIGHT) / LINE_HEIGHT).ceil() as usize;
        let bottom = (top_visible + lines_visible + 1).min(file.lines.len());
        let mut y = start_y - (win.scroll % LINE_HEIGHT);

        for idx in top_visible..bottom {
            let line = &file.lines[idx];
            let text_start_x = content_rect.x + CODE_X_OFFSET - win.scroll_x;
            font.draw_text_ex(
                d,
                &format!("{:>4}", idx + 1),
                Vector2::new(content_rect.x + 4.0, y),
                FONT_SIZE - 2.0,
                0.0,
                COLOR_LINE_NUM,
            );

            let calls: Vec<&FunctionCall> = file.calls_on_line(idx).collect();
            if calls.is_empty() {
                let segments = colorize_line(line, &[]);
                draw_segments(d, font, text_start_x, y, &segments);
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
        let segments = colorize_line(line, calls);
        draw_segments(d, font, base_x, y, &segments);
    }
}

fn colorize_line(line: &str, calls: &[&FunctionCall]) -> Vec<(String, Color)> {
    let mut segments: Vec<(String, Color)> = Vec::new();
    let mut i = 0;
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut call_ranges: Vec<(usize, usize)> = calls.iter().map(|c| (c.col, c.len)).collect();
    call_ranges.sort_by_key(|r| r.0);

    while i < len {
        if let Some(&(start, clen)) = call_ranges.iter().find(|&&(s, _)| s == i) {
            let text = line[start..start + clen].to_string();
            append_segment(&mut segments, text, COLOR_CALL);
            i += clen;
            continue;
        }

        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            let text = line[i..].to_string();
            append_segment(&mut segments, text, COLOR_COMMENT);
            break;
        }

        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            let mut escaped = false;
            while i < len {
                let b = bytes[i];
                if b == b'\\' && !escaped {
                    escaped = true;
                    i += 1;
                    continue;
                }
                if b == b'"' && !escaped {
                    i += 1;
                    break;
                }
                escaped = false;
                i += 1;
            }
            let text = line[start..i].to_string();
            append_segment(&mut segments, text, COLOR_STRING);
            continue;
        }

        let b = bytes[i];
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < len {
                let b = bytes[i];
                if b.is_ascii_alphanumeric() || b == b'_' {
                    i += 1;
                } else {
                    break;
                }
            }
            let word = &line[start..i];
            let color = if KEYWORDS.iter().any(|k| *k == word) {
                COLOR_KEYWORD
            } else {
                COLOR_TEXT
            };
            append_segment(&mut segments, word.to_string(), color);
            continue;
        }

        let ch = &line[i..i + 1];
        append_segment(&mut segments, ch.to_string(), COLOR_TEXT);
        i += 1;
    }

    segments
}

fn append_segment(segments: &mut Vec<(String, Color)>, text: String, color: Color) {
    if text.is_empty() {
        return;
    }
    if let Some((last_text, last_color)) = segments.last_mut() {
        if last_color == &color {
            last_text.push_str(&text);
            return;
        }
    }
    segments.push((text, color));
}

fn draw_segments(
    d: &mut RaylibDrawHandle,
    font: &AppFont,
    base_x: f32,
    y: f32,
    segments: &[(String, Color)],
) {
    let mut x = base_x;
    for (text, color) in segments {
        font.draw_text_ex(d, text, Vector2::new(x, y), FONT_SIZE, 0.0, *color);
        x += font.measure_width(text, FONT_SIZE, 0.0);
    }
}

fn estimated_line_width(line: &str) -> f32 {
    // Rough approximation for monospace fonts where glyph width ~0.6 * font size.
    line.chars().count() as f32 * FONT_SIZE * 0.6
}

struct SyntaxCollector<'a> {
    file: &'a Path,
    source: &'a str,
    defs: Vec<FunctionDef>,
    calls: Vec<FunctionCall>,
    module_stack: Vec<String>,
    keywords: HashSet<&'static str>,
}

impl<'a> SyntaxCollector<'a> {
    fn new(file: &'a Path, source: &'a str) -> Self {
        let keywords: HashSet<&'static str> = KEYWORDS.into_iter().collect();

        Self {
            file,
            source,
            defs: Vec::new(),
            calls: Vec::new(),
            module_stack: Vec::new(),
            keywords,
        }
    }

    fn module_path(&self) -> String {
        if self.module_stack.is_empty() {
            "crate".to_string()
        } else {
            format!("crate::{}", self.module_stack.join("::"))
        }
    }

    fn push_mod(&mut self, name: &str) {
        self.module_stack.push(name.to_string());
    }

    fn pop_mod(&mut self) {
        self.module_stack.pop();
    }

    fn add_def(&mut self, ident: &syn::Ident, span: Span) {
        if let Some((line, col)) = span_to_line_col(span) {
            self.defs.push(FunctionDef {
                name: ident.to_string(),
                module_path: self.module_path(),
                line,
                col,
            });
        }
    }

    fn add_call(&mut self, name: String, span: Span) {
        if let Some((line, col)) = span_to_line_col(span) {
            self.calls.push(FunctionCall {
                name: name.clone(),
                module_path: self.module_path(),
                line,
                col,
                len: span_to_len(span, self.source).unwrap_or(name.len()),
            });
        }
    }
}

impl<'ast> Visit<'ast> for SyntaxCollector<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.add_def(&node.sig.ident, node.sig.ident.span());
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.add_def(&node.sig.ident, node.sig.ident.span());
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        self.push_mod(&node.ident.to_string());
        syn::visit::visit_item_mod(self, node);
        self.pop_mod();
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(ref path) = *node.func {
            if let Some(segment) = path.path.segments.last() {
                let name = segment.ident.to_string();
                if !self.keywords.contains(name.as_str()) {
                    self.add_call(name, segment.ident.span());
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        if !self.keywords.contains(name.as_str()) {
            self.add_call(name, node.method.span());
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

fn span_to_line_col(span: Span) -> Option<(usize, usize)> {
    let start = span.start();
    Some((start.line.saturating_sub(1), start.column))
}

fn span_to_len(span: Span, source: &str) -> Option<usize> {
    let start = span.start();
    let end = span.end();
    if start.line != end.line {
        return None;
    }
    let line_idx = start.line.saturating_sub(1);
    let line = source.lines().nth(line_idx)?;
    let start_idx = start.column.min(line.len());
    let end_idx = end.column.min(line.len());
    Some(end_idx.saturating_sub(start_idx))
}

fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);

    let project = ProjectModel::load(&root)
        .with_context(|| format!("loading project at {}", root.display()))?;
    let (mut rl, thread) = raylib::init()
        .size(1280, 780)
        .resizable()
        .title("Rust Trace Viewer")
        .msaa_4x()
        .build();
    rl.set_target_fps(60);
    let font = load_monospace_font(&mut rl, &thread);

    let mut app = AppState::new(project);

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
        );

        let mut d = rl.begin_drawing(&thread);
        app.draw(&mut d, &font, mouse);
    }

    app.save_layout().context("saving layout")?;
    Ok(())
}

fn parse_rust_file(path: &Path) -> Result<ParsedFile> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

    let file = syn::parse_file(&content)?;
    let mut collector = SyntaxCollector::new(path, &content);
    collector.visit_file(&file);

    Ok(ParsedFile {
        path: path.to_path_buf(),
        lines,
        defs: collector.defs,
        calls: collector.calls,
    })
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
    let width_before = font.measure_width(before, FONT_SIZE, 0.0);
    let width_token = font.measure_width(token, FONT_SIZE, 0.0);
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

fn collect_typed_chars(rl: &mut RaylibHandle) -> String {
    let mut out = String::new();
    while let Some(ch) = rl.get_char_pressed() {
        if !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

fn load_monospace_font(rl: &mut RaylibHandle, thread: &RaylibThread) -> AppFont {
    let candidates = vec![
        env::var("TRACE_VIEWER_FONT").ok(),
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
