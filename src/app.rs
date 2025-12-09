use std::path::PathBuf;

use raylib::prelude::*;

use crate::code_window::{
    self, CodeViewKind, CodeWindow, MIN_WINDOW_H, MIN_WINDOW_W, SCROLLBAR_MIN_THUMB,
    SCROLLBAR_PADDING, SCROLLBAR_THICKNESS,
};
use crate::constants::{
    BREADCRUMB_HEIGHT, CODE_X_OFFSET, LAYOUT_FILE, LINE_HEIGHT, SIDEBAR_WIDTH, TITLE_BAR_HEIGHT,
};
use crate::helpers::matches_view;
use crate::icons::{Icon, Icons};
use crate::model::{
    DefinitionLocation, FunctionCall, ParsedFile, ProjectModel, colorize_line, find_function_span,
};
use crate::sidebar::{SidebarAction, SidebarState};
use crate::theme::{ColorKind, Palette};
use crate::{AppFont, FONT_SIZE, point_in_rect, token_rect};
use serde::{Deserialize, Serialize};

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
    SingleFn {
        start: usize,
        end: usize,
        title: String,
    },
}

pub struct AppState {
    pub project: ProjectModel,
    pub windows: Vec<CodeWindow>,
    pub next_window_id: usize,
    pub sidebar: SidebarState,
    pub palette: Palette,
    icons: Icons,
    pan: Vector2,
    pan_dragging: bool,
    pan_anchor: Vector2,
    pan_start: Vector2,
}

impl AppState {
    pub fn new(
        project: ProjectModel,
        rl: &mut RaylibHandle,
        thread: &RaylibThread,
        palette: Palette,
    ) -> Self {
        let icons = Icons::load(rl, thread, 16);
        let mut state = Self {
            project,
            windows: Vec::new(),
            next_window_id: 1,
            sidebar: SidebarState::with_icons(rl, thread),
            palette,
            icons,
            pan: Vector2::new(0.0, 0.0),
            pan_dragging: false,
            pan_anchor: Vector2::new(0.0, 0.0),
            pan_start: Vector2::new(0.0, 0.0),
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
                    Some(SavedViewKind::SingleFn {
                        start,
                        end,
                        title: t,
                    }) => {
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
                    is_resizing: false,
                    resize_origin_pos: Vector2 { x: 0.0, y: 0.0 },
                    resize_origin_size: Vector2 { x: 0.0, y: 0.0 },
                    resize_edges: (false, false, false, false),
                    hover_edges: None,
                    dragging_vscroll: false,
                    dragging_hscroll: false,
                    dragging_minimap: false,
                    drag_start: Vector2 { x: 0.0, y: 0.0 },
                };
                code_window::clamp_window_scroll(&self.project, &mut win);
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
            code_window::clamp_window_scroll(&self.project, &mut win);
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
            is_resizing: false,
            resize_origin_pos: Vector2 { x: 0.0, y: 0.0 },
            resize_origin_size: Vector2 { x: 0.0, y: 0.0 },
            resize_edges: (false, false, false, false),
            hover_edges: None,
            dragging_vscroll: false,
            dragging_hscroll: false,
            dragging_minimap: false,
            drag_start: Vector2 { x: 0.0, y: 0.0 },
        };
        code_window::clamp_window_scroll(&self.project, &mut win);
        self.next_window_id += 1;
        self.windows.push(win);
    }

    fn max_scroll_x(&self, win: &CodeWindow) -> Option<f32> {
        code_window::metrics_for(&self.project, win).map(|m| m.max_scroll_x())
    }

    pub fn handle_input(
        &mut self,
        font: &AppFont,
        mouse: Vector2,
        wheel: f32,
        left_pressed: bool,
        left_down: bool,
        middle_pressed: bool,
        middle_down: bool,
        typed: String,
        backspace: bool,
        shift_down: bool,
        sidebar_height: f32,
    ) {
        let world_mouse = Vector2::new(mouse.x - self.pan.x, mouse.y - self.pan.y);

        if middle_pressed {
            self.pan_dragging = true;
            self.pan_anchor = mouse;
            self.pan_start = self.pan;
        }
        if !middle_down {
            self.pan_dragging = false;
        }

        if self.pan_dragging {
            let dx = mouse.x - self.pan_anchor.x;
            let dy = mouse.y - self.pan_anchor.y;
            self.pan = Vector2::new(self.pan_start.x + dx, self.pan_start.y + dy);
            return;
        }

        if !left_down {
            for w in &mut self.windows {
                w.is_dragging = false;
                w.is_resizing = false;
                w.dragging_vscroll = false;
                w.dragging_hscroll = false;
                w.dragging_minimap = false;
                w.hover_edges = None;
            }
        }

        if wheel.abs() > f32::EPSILON {
            if self.sidebar.handle_wheel(
                mouse,
                wheel,
                &self.project,
                &self.project.defs,
                sidebar_height,
            ) {
                return;
            }
            for idx in (0..self.windows.len()).rev() {
                let content = self.windows[idx].content_rect_at(self.pan);
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
                    w.position = Vector2::new(
                        world_mouse.x - w.drag_offset.x,
                        world_mouse.y - w.drag_offset.y,
                    );
                }
                if w.is_resizing {
                    let (left, right, top, bottom) = w.resize_edges;
                    let mut new_pos = w.resize_origin_pos;
                    let mut new_size = w.resize_origin_size;
                    let dx = world_mouse.x - w.drag_start.x;
                    let dy = world_mouse.y - w.drag_start.y;
                    if left {
                        let max_x = w.resize_origin_pos.x + w.resize_origin_size.x - MIN_WINDOW_W;
                        let nx = (w.resize_origin_pos.x + dx).min(max_x);
                        new_size.x =
                            (w.resize_origin_pos.x + w.resize_origin_size.x - nx).max(MIN_WINDOW_W);
                        new_pos.x = nx;
                    }
                    if right {
                        new_size.x = (w.resize_origin_size.x + dx).max(MIN_WINDOW_W);
                    }
                    if top {
                        let max_y = w.resize_origin_pos.y + w.resize_origin_size.y - MIN_WINDOW_H;
                        let ny = (w.resize_origin_pos.y + dy).min(max_y);
                        new_size.y =
                            (w.resize_origin_pos.y + w.resize_origin_size.y - ny).max(MIN_WINDOW_H);
                        new_pos.y = ny;
                    }
                    if bottom {
                        new_size.y = (w.resize_origin_size.y + dy).max(MIN_WINDOW_H);
                    }
                    w.position = new_pos;
                    w.size = new_size;
                    code_window::clamp_window_scroll(&self.project, w);
                }
                if w.dragging_vscroll {
                    if let Some(metrics) = code_window::metrics_for(&self.project, w) {
                        let track_y =
                            w.position.y + self.pan.y + TITLE_BAR_HEIGHT + BREADCRUMB_HEIGHT;
                        let track_h = metrics.avail_height;
                        if track_h > 0.0 {
                            let thumb_h = (metrics.avail_height / metrics.total_height.max(1.0)
                                * track_h)
                                .clamp(SCROLLBAR_MIN_THUMB, track_h);
                            let denom = (track_h - thumb_h).max(1.0);
                            let scroll_range = metrics.max_scroll_y();
                            let ratio =
                                ((mouse.y - track_y - w.drag_start.y) / denom).clamp(0.0, 1.0);
                            w.scroll = (ratio * scroll_range).clamp(0.0, scroll_range);
                        }
                    }
                }
                if w.dragging_hscroll {
                    if let Some(metrics) = code_window::metrics_for(&self.project, w) {
                        let track_x = w.position.x + self.pan.x + CODE_X_OFFSET;
                        let track_w = metrics.avail_width;
                        if track_w > 0.0 {
                            let thumb_w = (metrics.avail_width / metrics.max_width.max(1.0)
                                * track_w)
                                .clamp(SCROLLBAR_MIN_THUMB, track_w);
                            let denom = (track_w - thumb_w).max(1.0);
                            let scroll_range = metrics.max_scroll_x();
                            let ratio =
                                ((mouse.x - track_x - w.drag_start.x) / denom).clamp(0.0, 1.0);
                            w.scroll_x = (ratio * scroll_range).clamp(0.0, scroll_range);
                        }
                    }
                }
                if w.dragging_minimap {
                    if let Some(metrics) = code_window::metrics_for(&self.project, w) {
                        if let Some(mini) = w.minimap_rect_at(&metrics, self.pan) {
                            let ratio = ((mouse.y - mini.y) / mini.height).clamp(0.0, 1.0);
                            let scroll_range = metrics.max_scroll_y();
                            w.scroll = (ratio * scroll_range).clamp(0.0, scroll_range);
                        }
                    }
                }
                if !w.is_resizing {
                    w.hover_edges = w.hit_resize_edges(mouse, self.pan);
                }
            }
        }

        if left_pressed {
            let mut handled = false;
            for idx in (0..self.windows.len()).rev() {
                if self.windows[idx].hit_test(world_mouse) {
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
                                win.is_resizing = false;
                            }
                        }
                        WindowAction::StartResize { edges } => {
                            if let Some(win) = self.windows.last_mut() {
                                win.is_resizing = true;
                                win.resize_origin_pos = win.position;
                                win.resize_origin_size = win.size;
                                win.resize_edges = edges;
                                win.drag_start = world_mouse;
                                win.is_dragging = false;
                                win.hover_edges = Some(edges);
                            }
                        }
                        WindowAction::StartVScroll { grab_offset, ratio } => {
                            if let Some(win) = self.windows.last_mut() {
                                win.dragging_vscroll = true;
                                win.drag_start.y = grab_offset;
                                if let Some(metrics) = code_window::metrics_for(&self.project, win)
                                {
                                    let scroll_range = metrics.max_scroll_y();
                                    win.scroll = (ratio * scroll_range).clamp(0.0, scroll_range);
                                }
                                win.dragging_hscroll = false;
                                win.dragging_minimap = false;
                            }
                        }
                        WindowAction::StartHScroll { grab_offset, ratio } => {
                            if let Some(win) = self.windows.last_mut() {
                                win.dragging_hscroll = true;
                                win.drag_start.x = grab_offset;
                                if let Some(metrics) = code_window::metrics_for(&self.project, win)
                                {
                                    let scroll_range = metrics.max_scroll_x();
                                    win.scroll_x = (ratio * scroll_range).clamp(0.0, scroll_range);
                                }
                                win.dragging_vscroll = false;
                                win.dragging_minimap = false;
                            }
                        }
                        WindowAction::StartMinimap { ratio } => {
                            if let Some(win) = self.windows.last_mut() {
                                win.dragging_minimap = true;
                                win.drag_start.y = ratio;
                                if let Some(metrics) = code_window::metrics_for(&self.project, win)
                                {
                                    let scroll_range = metrics.max_scroll_y();
                                    win.scroll = (ratio * scroll_range).clamp(0.0, scroll_range);
                                }
                                win.dragging_vscroll = false;
                                win.dragging_hscroll = false;
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

        let resizing_idx = self.windows.iter().position(|w| w.is_resizing);
        if let Some(active_idx) = resizing_idx {
            for (i, w) in self.windows.iter_mut().enumerate() {
                if i == active_idx {
                    w.hover_edges = Some(w.resize_edges);
                } else {
                    w.hover_edges = None;
                }
            }
        } else {
            for w in &mut self.windows {
                w.hover_edges = None;
            }
            for idx in (0..self.windows.len()).rev() {
                let can_hover = {
                    let w = &self.windows[idx];
                    !w.is_dragging
                        && !w.dragging_vscroll
                        && !w.dragging_hscroll
                        && !w.dragging_minimap
                };
                if !can_hover {
                    continue;
                }
                let edges = self.windows[idx].hit_resize_edges(mouse, self.pan);
                if let Some(edges) = edges {
                    if let Some(w) = self.windows.get_mut(idx) {
                        w.hover_edges = Some(edges);
                    }
                    break;
                }
            }
        }
    }

    fn max_scroll(&self, idx: usize) -> f32 {
        code_window::metrics_for(&self.project, &self.windows[idx])
            .map(|m| m.max_scroll_y())
            .unwrap_or(0.0)
    }

    pub fn draw(&mut self, d: &mut RaylibDrawHandle, font: &AppFont, mouse: Vector2) {
        let mut hover_cursor: Option<MouseCursor> = None;
        d.clear_background(self.palette.bg);
        for idx in 0..self.windows.len() {
            if let Some(edges) = self.windows[idx].hover_edges {
                hover_cursor = Some(cursor_for_edges(edges));
            }
            let is_top = idx + 1 == self.windows.len();
            self.draw_window(d, font, idx, is_top);
        }
        d.set_mouse_cursor(hover_cursor.unwrap_or(MouseCursor::MOUSE_CURSOR_DEFAULT));
        self.sidebar.draw(
            d,
            font,
            mouse,
            &self.project,
            &self.project.defs,
            &self.palette,
        );
    }

    fn draw_window(&self, d: &mut RaylibDrawHandle, font: &AppFont, idx: usize, is_top: bool) {
        let win = &self.windows[idx];
        let bg = if is_top {
            self.palette.window_top
        } else {
            self.palette.window
        };
        let radius = 0.01;
        let win_rect = win.rect_at(self.pan);
        d.draw_rectangle_rounded(win_rect, radius, 10, bg);
        d.draw_rectangle_rounded_lines(win_rect, radius, 10, self.palette.breadcrumb);

        let title_rect = win.title_rect_at(self.pan);
        d.draw_rectangle_rounded(
            Rectangle {
                x: title_rect.x,
                y: title_rect.y,
                width: title_rect.width,
                height: title_rect.height,
            },
            radius,
            12,
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
        let icon_size = self.icons.size() as f32;
        let close_rect = Rectangle {
            x: title_rect.x + title_rect.width - icon_size - 8.0,
            y: title_rect.y + (TITLE_BAR_HEIGHT - icon_size) * 0.5,
            width: icon_size,
            height: icon_size,
        };
        self.icons.render(
            d,
            Icon::Close,
            Vector2::new(close_rect.x, close_rect.y),
            self.palette.close,
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
        let content_rect = win.content_rect_at(self.pan);
        if content_rect.width <= 0.0 || content_rect.height <= 0.0 {
            return;
        }

        let metrics = code_window::content_metrics(file, win);
        let scissor_w = (content_rect.width
            - if metrics.show_v {
                SCROLLBAR_THICKNESS + SCROLLBAR_PADDING
            } else {
                0.0
            })
        .max(1.0);
        let scissor_h = (content_rect.height
            - if metrics.show_h {
                SCROLLBAR_THICKNESS + SCROLLBAR_PADDING
            } else {
                0.0
            })
        .max(1.0);
        let mut scoped = d.begin_scissor_mode(
            content_rect.x as i32,
            content_rect.y as i32,
            scissor_w as i32,
            scissor_h as i32,
        );

        let mut breadcrumb = self.project.display_name(&file.path);
        if let Some(mod_path) = file.defs.first().map(|d| d.module_path.as_str()) {
            breadcrumb.push_str(" - ");
            breadcrumb.push_str(mod_path);
        }
        font.draw_text_ex(
            &mut scoped,
            &breadcrumb,
            Vector2::new(content_rect.x + 8.0, content_rect.y + 2.0),
            FONT_SIZE - 2.0,
            0.0,
            self.palette.breadcrumb,
        );

        let start_y = content_rect.y + BREADCRUMB_HEIGHT;
        let text_area_height = metrics.avail_height;
        let top_visible = (win.scroll / LINE_HEIGHT).floor() as usize;
        let lines_visible = ((text_area_height + LINE_HEIGHT) / LINE_HEIGHT).ceil() as usize;
        let bottom = (top_visible + lines_visible + 1).min(file.lines.len());
        let mut y = start_y - (win.scroll % LINE_HEIGHT);

        let gutter_width = CODE_X_OFFSET - 4.0;
        scoped.draw_rectangle(
            content_rect.x as i32,
            start_y as i32,
            gutter_width as i32,
            (metrics.avail_height + LINE_HEIGHT) as i32,
            self.palette.window,
        );

        for idx in top_visible..bottom {
            let line = &file.lines[idx];
            let text_start_x = content_rect.x + CODE_X_OFFSET - win.scroll_x;

            let calls: Vec<&FunctionCall> = file.calls_on_line(idx).collect();
            if calls.is_empty() {
                let segments = colorize_line(line, &[]);
                crate::draw_segments(&mut scoped, font, text_start_x, y, &segments, &self.palette);
            } else {
                let segments = colorize_line(line, &calls);
                crate::draw_segments(&mut scoped, font, text_start_x, y, &segments, &self.palette);
            }

            font.draw_text_ex(
                &mut scoped,
                &format!("{:>4}", idx + 1),
                Vector2::new(content_rect.x + 4.0, y),
                FONT_SIZE - 2.0,
                0.0,
                self.palette.line_num,
            );
            y += LINE_HEIGHT;
        }
        drop(scoped);

        if let Some(mini) = win.minimap_rect_at(&metrics, self.pan) {
            d.draw_rectangle(
                mini.x as i32,
                mini.y as i32,
                mini.width as i32,
                mini.height as i32,
                self.palette.window,
            );
            let scale = mini.height / metrics.total_height.max(1.0);
            let line_scale = (scale * LINE_HEIGHT).max(1.0);
            let block_height = (line_scale * 0.7).clamp(1.0, line_scale);
            let max_width = (mini.width - 4.0).max(1.0);
            let mut scoped = d.begin_scissor_mode(
                mini.x as i32,
                mini.y as i32,
                mini.width as i32,
                mini.height as i32,
            );
            for (idx, line) in file.lines.iter().enumerate() {
                let line_y = mini.y + idx as f32 * line_scale;
                if line_y > mini.y + mini.height {
                    break;
                }
                let calls: Vec<&FunctionCall> = file.calls_on_line(idx).collect();
                let segments = colorize_line(line, &calls);
                let mut x = mini.x + 2.0;
                for (text, color) in segments {
                    let width = font
                        .measure_width(&text, FONT_SIZE, 0.0)
                        .max(text.len() as f32 * 4.0);
                    let w = (width / metrics.max_width.max(1.0)) * max_width;
                    if w <= 0.5 {
                        continue;
                    }
                    let c = match color {
                        ColorKind::Text => self.palette.text,
                        ColorKind::Comment => self.palette.comment,
                        ColorKind::String => self.palette.string,
                        ColorKind::Keyword => self.palette.keyword,
                        ColorKind::Call => self.palette.call,
                    };
                    scoped.draw_rectangle(
                        x as i32,
                        line_y as i32,
                        w as i32,
                        block_height as i32,
                        c,
                    );
                    x += w;
                    if x > mini.x + mini.width - 2.0 {
                        break;
                    }
                }
            }
            drop(scoped);

            let view_h = (metrics.avail_height * scale).clamp(4.0, mini.height);
            let view_y = mini.y + win.scroll * scale;
            d.draw_rectangle(
                mini.x as i32,
                view_y as i32,
                mini.width as i32,
                view_h as i32,
                self.palette.sidebar_highlight,
            );
            d.draw_rectangle_lines(
                mini.x as i32,
                mini.y as i32,
                mini.width as i32,
                mini.height as i32,
                self.palette.title,
            );
        }

        if metrics.show_v {
            let track_x =
                content_rect.x + content_rect.width - SCROLLBAR_THICKNESS - SCROLLBAR_PADDING;
            let track_y = content_rect.y + BREADCRUMB_HEIGHT;
            let track_h = metrics.avail_height;
            d.draw_rectangle(
                track_x as i32,
                track_y as i32,
                SCROLLBAR_THICKNESS as i32,
                track_h as i32,
                self.palette.search_bg,
            );
            let scroll_range = metrics.max_scroll_y().max(1.0);
            let denom = metrics.total_height.max(1.0);
            let thumb_h =
                (metrics.avail_height / denom * track_h).clamp(SCROLLBAR_MIN_THUMB, track_h);
            let thumb_y = track_y + (win.scroll / scroll_range) * (track_h - thumb_h);
            d.draw_rectangle(
                track_x as i32,
                thumb_y as i32,
                SCROLLBAR_THICKNESS as i32,
                thumb_h as i32,
                self.palette.sidebar_highlight,
            );
        }

        if metrics.show_h {
            let track_x = content_rect.x + CODE_X_OFFSET;
            let track_y =
                content_rect.y + content_rect.height - SCROLLBAR_THICKNESS - SCROLLBAR_PADDING;
            let track_w = metrics.avail_width;
            d.draw_rectangle(
                track_x as i32,
                track_y as i32,
                track_w as i32,
                SCROLLBAR_THICKNESS as i32,
                self.palette.search_bg,
            );
            let scroll_range = metrics.max_scroll_x().max(1.0);
            let denom = metrics.max_width.max(1.0);
            let thumb_w =
                (metrics.avail_width / denom * track_w).clamp(SCROLLBAR_MIN_THUMB, track_w);
            let thumb_x = track_x + (win.scroll_x / scroll_range) * (track_w - thumb_w);
            d.draw_rectangle(
                thumb_x as i32,
                track_y as i32,
                thumb_w as i32,
                SCROLLBAR_THICKNESS as i32,
                self.palette.sidebar_highlight,
            );
        }
    }

    fn handle_window_click(&mut self, font: &AppFont, idx: usize, mouse: Vector2) -> WindowAction {
        let world_mouse = Vector2::new(mouse.x - self.pan.x, mouse.y - self.pan.y);
        let win = &self.windows[idx];
        let title_rect = win.title_rect_at(self.pan);
        let icon_size = self.icons.size() as f32;
        let close_rect = Rectangle {
            x: title_rect.x + title_rect.width - icon_size - 8.0,
            y: title_rect.y + (TITLE_BAR_HEIGHT - icon_size) * 0.5,
            width: icon_size,
            height: icon_size,
        };

        if point_in_rect(mouse, close_rect) {
            return WindowAction::Close;
        }

        if let Some(edges) = win.hit_resize_edges(mouse, self.pan) {
            return WindowAction::StartResize { edges };
        }

        if point_in_rect(mouse, title_rect) {
            return WindowAction::StartDrag(Vector2 {
                x: world_mouse.x - win.position.x,
                y: world_mouse.y - win.position.y,
            });
        }

        let content_rect = win.content_rect_at(self.pan);
        if !point_in_rect(mouse, content_rect) {
            return WindowAction::None;
        }

        if let Some(pf) = self.project.parsed.get(&win.file) {
            let metrics = code_window::content_metrics(pf, win);
            if let Some(mini) = win.minimap_rect_at(&metrics, self.pan) {
                if point_in_rect(mouse, mini) {
                    let ratio = ((mouse.y - mini.y) / mini.height).clamp(0.0, 1.0);
                    return WindowAction::StartMinimap { ratio };
                }
            }
            if metrics.show_v {
                let track_y = content_rect.y + BREADCRUMB_HEIGHT;
                let track_h = metrics.avail_height;
                if track_h > 0.0 {
                    let track_x = content_rect.x + content_rect.width
                        - SCROLLBAR_THICKNESS
                        - SCROLLBAR_PADDING;
                    let scroll_range = metrics.max_scroll_y();
                    let thumb_h = (metrics.avail_height / metrics.total_height.max(1.0) * track_h)
                        .clamp(SCROLLBAR_MIN_THUMB, track_h);
                    let thumb_y = track_y
                        + if scroll_range > 0.0 {
                            (win.scroll / scroll_range) * (track_h - thumb_h)
                        } else {
                            0.0
                        };
                    let denom = (track_h - thumb_h).max(1.0);
                    let ratio = ((mouse.y - track_y - thumb_h * 0.5) / denom).clamp(0.0, 1.0);
                    let grab_offset = (mouse.y - thumb_y).clamp(0.0, thumb_h);
                    let v_rect = Rectangle {
                        x: track_x,
                        y: track_y,
                        width: SCROLLBAR_THICKNESS,
                        height: track_h,
                    };
                    if point_in_rect(mouse, v_rect) {
                        return WindowAction::StartVScroll { grab_offset, ratio };
                    }
                }
            }
            if metrics.show_h {
                let track_x = content_rect.x + CODE_X_OFFSET;
                let track_w = metrics.avail_width;
                if track_w > 0.0 {
                    let track_y = content_rect.y + content_rect.height
                        - SCROLLBAR_THICKNESS
                        - SCROLLBAR_PADDING;
                    let scroll_range = metrics.max_scroll_x();
                    let thumb_w = (metrics.avail_width / metrics.max_width.max(1.0) * track_w)
                        .clamp(SCROLLBAR_MIN_THUMB, track_w);
                    let thumb_x = track_x
                        + if scroll_range > 0.0 {
                            (win.scroll_x / scroll_range) * (track_w - thumb_w)
                        } else {
                            0.0
                        };
                    let denom = (track_w - thumb_w).max(1.0);
                    let ratio = ((mouse.x - track_x - thumb_w * 0.5) / denom).clamp(0.0, 1.0);
                    let grab_offset = (mouse.x - thumb_x).clamp(0.0, thumb_w);
                    let h_rect = Rectangle {
                        x: track_x,
                        y: track_y,
                        width: track_w,
                        height: SCROLLBAR_THICKNESS,
                    };
                    if point_in_rect(mouse, h_rect) {
                        return WindowAction::StartHScroll { grab_offset, ratio };
                    }
                }
            }
            if metrics.show_v {
                let v_rect = Rectangle {
                    x: content_rect.x + content_rect.width
                        - SCROLLBAR_THICKNESS
                        - SCROLLBAR_PADDING,
                    y: content_rect.y + BREADCRUMB_HEIGHT,
                    width: SCROLLBAR_THICKNESS,
                    height: metrics.avail_height,
                };
                if point_in_rect(mouse, v_rect) {
                    return WindowAction::None;
                }
            }
            if metrics.show_h {
                let h_rect = Rectangle {
                    x: content_rect.x + CODE_X_OFFSET,
                    y: content_rect.y + content_rect.height
                        - SCROLLBAR_THICKNESS
                        - SCROLLBAR_PADDING,
                    width: metrics.avail_width,
                    height: SCROLLBAR_THICKNESS,
                };
                if point_in_rect(mouse, h_rect) {
                    return WindowAction::None;
                }
            }
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
        let content_rect = win.content_rect_at(self.pan);
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
            code_window::clamp_window_scroll(&self.project, &mut win);
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
            is_resizing: false,
            resize_origin_pos: Vector2 { x: 0.0, y: 0.0 },
            resize_origin_size: Vector2 { x: 0.0, y: 0.0 },
            resize_edges: (false, false, false, false),
            hover_edges: None,
            dragging_vscroll: false,
            dragging_hscroll: false,
            dragging_minimap: false,
            drag_start: Vector2 { x: 0.0, y: 0.0 },
        };
        code_window::clamp_window_scroll(&self.project, &mut win);
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
    StartResize { edges: (bool, bool, bool, bool) },
    StartVScroll { grab_offset: f32, ratio: f32 },
    StartHScroll { grab_offset: f32, ratio: f32 },
    StartMinimap { ratio: f32 },
}

fn cursor_for_edges(edges: (bool, bool, bool, bool)) -> MouseCursor {
    let (l, r, t, b) = edges;
    match (l, r, t, b) {
        (true, false, true, false) | (false, true, false, true) => {
            MouseCursor::MOUSE_CURSOR_RESIZE_NWSE
        }
        (true, false, false, true) | (false, true, true, false) => {
            MouseCursor::MOUSE_CURSOR_RESIZE_NESW
        }
        (true, false, false, false) | (false, true, false, false) => {
            MouseCursor::MOUSE_CURSOR_RESIZE_EW
        }
        (false, false, true, false) | (false, false, false, true) => {
            MouseCursor::MOUSE_CURSOR_RESIZE_NS
        }
        _ => MouseCursor::MOUSE_CURSOR_DEFAULT,
    }
}
