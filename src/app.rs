use std::path::{Path, PathBuf};

use raylib::prelude::*;

use crate::constants::{
    BREADCRUMB_HEIGHT, CODE_X_OFFSET, LAYOUT_FILE, LINE_HEIGHT, SIDEBAR_WIDTH, TITLE_BAR_HEIGHT,
};
use crate::helpers::matches_view;
use crate::model::{
    colorize_line, find_function_span, DefinitionLocation, FunctionCall, ParsedFile, ProjectModel,
};
use crate::sidebar::{SidebarAction, SidebarState};
use crate::theme::Palette;
use crate::{point_in_rect, token_rect, AppFont, FONT_SIZE};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct CodeWindow {
    pub id: usize,
    pub file: PathBuf,
    pub title: String,
    pub focus_line: Option<usize>,
    pub view_kind: CodeViewKind,
    pub position: Vector2,
    pub size: Vector2,
    pub scroll: f32,
    pub scroll_x: f32,
    pub is_dragging: bool,
    pub drag_offset: Vector2,
}

#[derive(Clone, Debug)]
pub enum CodeViewKind {
    FullFile,
    SingleFn { start: usize, end: usize },
}

#[derive(Serialize, Deserialize)]
struct SavedWindow {
    file: String,
    view_kind: Option<SavedViewKind>,
    position: (f32, f32),
    size: (f32, f32),
    scroll: f32,
    #[serde(default)]
    scroll_x: f32,
}

#[derive(Serialize, Deserialize)]
struct SavedLayout {
    windows: Vec<SavedWindow>,
    #[serde(default)]
    sidebar_scroll: f32,
    #[serde(default)]
    sidebar_collapsed: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum SavedViewKind {
    FullFile,
    SingleFn { start: usize, end: usize, title: String },
}

pub struct AppState {
    pub project: ProjectModel,
    pub windows: Vec<CodeWindow>,
    pub next_window_id: usize,
    pub sidebar: SidebarState,
    pub palette: Palette,
}

impl AppState {
    pub fn new(project: ProjectModel, rl: &mut RaylibHandle, thread: &RaylibThread, palette: Palette) -> Self {
        let mut state = Self {
            project,
            windows: Vec::new(),
            next_window_id: 1,
            sidebar: SidebarState::with_icons(rl, thread),
            palette,
        };
        state.load_layout();
        if state.windows.is_empty() {
            if let Some(first) = state.project.files.first() {
                state.open_file(first.clone(), None);
            }
        }
        state
    }

    fn layout_path(&self) -> PathBuf {
        self.project.root.join(LAYOUT_FILE)
    }

    fn load_layout(&mut self) {
        let path = self.layout_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        if let Ok(layout) = serde_json::from_str::<SavedLayout>(&text) {
            self.sidebar.scroll = layout.sidebar_scroll;
            self.sidebar.collapsed_dirs = layout
                .sidebar_collapsed
                .into_iter()
                .map(PathBuf::from)
                .collect();
            for saved in layout.windows {
                let file = self.project.root.join(&saved.file);
                if !self.project.parsed.contains_key(&file) {
                    continue;
                }
                let mut title = file
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("file")
                    .to_string();
                let view_kind = match saved.view_kind {
                    Some(SavedViewKind::SingleFn { start, end, title: t }) => {
                        title = t;
                        CodeViewKind::SingleFn { start, end }
                    }
                    _ => CodeViewKind::FullFile,
                };
                let mut win = CodeWindow {
                    id: self.next_window_id,
                    file,
                    title,
                    focus_line: None,
                    view_kind,
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

    pub fn save_layout(&self) -> anyhow::Result<()> {
        let windows: Vec<SavedWindow> = self
            .windows
            .iter()
            .map(|w| SavedWindow {
                file: self.project.display_name(&w.file),
                view_kind: match w.view_kind {
                    CodeViewKind::FullFile => None,
                    CodeViewKind::SingleFn { start, end } => Some(SavedViewKind::SingleFn {
                        start,
                        end,
                        title: w.title.clone(),
                    }),
                },
                position: (w.position.x, w.position.y),
                size: (w.size.x, w.size.y),
                scroll: w.scroll,
                scroll_x: w.scroll_x,
            })
            .collect();
        let mut sidebar_collapsed: Vec<String> = self
            .sidebar
            .collapsed_dirs
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        sidebar_collapsed.sort();
        sidebar_collapsed.dedup();
        let layout = SavedLayout {
            windows,
            sidebar_scroll: self.sidebar.scroll,
            sidebar_collapsed,
        };
        let path = self.layout_path();
        let text = serde_json::to_string_pretty(&layout)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn open_file(&mut self, path: PathBuf, jump_to: Option<usize>) {
        if let Some(idx) = self.windows.iter().position(|w| w.file == path) {
            let mut win = self.windows.remove(idx);
            if let Some(line) = jump_to {
                win.scroll = (line as f32 * LINE_HEIGHT - 40.0).max(0.0);
                win.focus_line = Some(line);
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
            focus_line: jump_to,
            view_kind: CodeViewKind::FullFile,
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
            let w = crate::estimated_line_width(line);
            if w > max_width {
                max_width = w;
            }
        }
        let visible = (win.size.x - CODE_X_OFFSET - 24.0).max(32.0);
        Some((max_width - visible).max(0.0))
    }

    pub fn handle_input(
        &mut self,
        font: &AppFont,
        mouse: Vector2,
        wheel: f32,
        left_pressed: bool,
        left_down: bool,
        typed: String,
        backspace: bool,
        shift_down: bool,
        sidebar_height: f32,
    ) {
        if !left_down {
            for w in &mut self.windows {
                w.is_dragging = false;
            }
        }

        if wheel.abs() > f32::EPSILON {
            if self
                .sidebar
                .handle_wheel(mouse, wheel, &self.project, &self.project.defs, sidebar_height)
            {
                return;
            }
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

        if left_down {
            for w in &mut self.windows {
                if w.is_dragging {
                    w.position = Vector2::new(mouse.x - w.drag_offset.x, mouse.y - w.drag_offset.y);
                }
            }
        }

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
                            self.open_definition(def);
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
                if let Some(action) =
                    self.sidebar
                        .handle_click(mouse, &self.project, &self.project.defs)
                {
                    match action {
                        SidebarAction::OpenFile { path, line } => self.open_file(path, line),
                        SidebarAction::ToggleDir => {}
                    }
                    handled = true;
                }
            }

            if !handled && point_in_rect(mouse, self.sidebar.search_rect()) {
                self.sidebar.search_focused = true;
            } else if !handled {
                self.sidebar.search_focused = false;
            }
        }

        if self.sidebar.search_focused {
            if backspace {
                self.sidebar.search_query.pop();
            }
            if !typed.is_empty() {
                self.sidebar.search_query.push_str(&typed);
            }
        }
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

    pub fn draw(&mut self, d: &mut RaylibDrawHandle, font: &AppFont, mouse: Vector2) {
        d.clear_background(self.palette.bg);
        self.sidebar
            .draw(d, font, mouse, &self.project, &self.project.defs, &self.palette);
        for idx in 0..self.windows.len() {
            let is_top = idx + 1 == self.windows.len();
            self.draw_window(d, font, idx, is_top);
        }
    }

    fn draw_window(&self, d: &mut RaylibDrawHandle, font: &AppFont, idx: usize, is_top: bool) {
        let win = &self.windows[idx];
        let bg = if is_top {
            self.palette.window_top
        } else {
            self.palette.window
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
            self.palette.title,
        );
        font.draw_text_ex(
            d,
            &win.title,
            Vector2::new(title_rect.x + 8.0, title_rect.y + 8.0),
            FONT_SIZE,
            0.0,
            self.palette.text,
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
            self.palette.close,
        );
        font.draw_text_ex(
            d,
            "x",
            Vector2::new(close_rect.x + 3.0, close_rect.y - 1.0),
            FONT_SIZE,
            0.0,
            self.palette.close,
        );

        if let Some(pf) = self.project.parsed.get(&win.file) {
            self.draw_code(d, font, pf, win);
        }
    }

    fn draw_code(&self, d: &mut RaylibDrawHandle, font: &AppFont, file: &ParsedFile, win: &CodeWindow) {
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
            self.palette.breadcrumb,
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
                self.palette.line_num,
            );

            let calls: Vec<&FunctionCall> = file.calls_on_line(idx).collect();
            if calls.is_empty() {
                let segments = colorize_line(line, &[]);
                crate::draw_segments(d, font, text_start_x, y, &segments, &self.palette);
            } else {
                let segments = colorize_line(line, &calls);
                crate::draw_segments(d, font, text_start_x, y, &segments, &self.palette);
            }

            y += LINE_HEIGHT;
        }
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
            let rect = crate::token_rect(
                font,
                line,
                call.col,
                call.len,
                content_rect.x + CODE_X_OFFSET - win.scroll_x,
                content_top + (line_idx as f32 * LINE_HEIGHT) - win.scroll,
            );
            if crate::point_in_rect(mouse, rect) {
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

    fn bring_to_front(&mut self, idx: usize) {
        if idx + 1 == self.windows.len() {
            return;
        }
        let w = self.windows.remove(idx);
        self.windows.push(w);
    }

    fn open_definition(&mut self, def: DefinitionLocation) {
        let title = def
            .module_path
            .split("::")
            .last()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                def.file
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("fn")
                    .to_string()
            });

        let view_kind = if let Some(pf) = self.project.parsed.get(&def.file) {
            if let Some((start, end)) = find_function_span(pf, def.line) {
                CodeViewKind::SingleFn { start, end }
            } else {
                CodeViewKind::FullFile
            }
        } else {
            CodeViewKind::FullFile
        };

        self.open_file_with_view(def.file, Some(def.line), title, view_kind);
    }

    fn open_file_with_view(
        &mut self,
        path: PathBuf,
        jump_to: Option<usize>,
        title: String,
        view_kind: CodeViewKind,
    ) {
        if let Some(idx) = self
            .windows
            .iter()
            .position(|w| w.file == path && matches_view(&w.view_kind, &view_kind))
        {
            let mut win = self.windows.remove(idx);
            if let Some(line) = jump_to {
                win.scroll = (line as f32 * LINE_HEIGHT - 40.0).max(0.0);
                win.focus_line = Some(line);
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
            title,
            focus_line: jump_to,
            view_kind,
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
}

#[derive(Clone, Debug)]
enum WindowAction {
    None,
    Close,
    OpenDefinition(DefinitionLocation),
    StartDrag(Vector2),
}

impl CodeWindow {
    pub fn content_rect(&self) -> Rectangle {
        Rectangle {
            x: self.position.x,
            y: self.position.y + TITLE_BAR_HEIGHT,
            width: self.size.x,
            height: self.size.y - TITLE_BAR_HEIGHT,
        }
    }

    pub fn title_rect(&self) -> Rectangle {
        Rectangle {
            x: self.position.x,
            y: self.position.y,
            width: self.size.x,
            height: TITLE_BAR_HEIGHT,
        }
    }
}
