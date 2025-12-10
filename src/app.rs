use std::path::PathBuf;

use raylib::prelude::*;

use crate::code_window::{
    self, CallOrigin, CodeViewKind, CodeWindow, MIN_WINDOW_H, MIN_WINDOW_W, SCROLLBAR_MIN_THUMB,
    SCROLLBAR_PADDING, SCROLLBAR_THICKNESS,
};
use crate::constants::{
    BREADCRUMB_HEIGHT, CODE_X_OFFSET, LAYOUT_FILE, LINE_HEIGHT, SIDEBAR_WIDTH, TITLE_BAR_HEIGHT,
};
use crate::helpers::matches_view;
use std::time::{Duration, Instant};
use crate::icons::Icons;
use crate::model::{DefinitionLocation, ProjectModel, find_function_span};
use crate::sidebar::{SidebarAction, SidebarState};
use crate::theme::Palette;
use crate::{AppFont, point_in_rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
struct CallLink {
    points: [Vector2; 4],
    caller_idx: usize,
    line: usize,
    hovered: bool,
    target_idx: usize,
}

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 1.0;
const MINIMAP_W: f32 = 220.0;
const MINIMAP_H: f32 = 160.0;
const MINIMAP_MARGIN: f32 = 10.0;
const MINIMAP_PAD: f32 = 8.0;
const MINIMAP_BTN_W: f32 = 56.0;
const MINIMAP_BTN_H: f32 = 18.0;
const MINIMAP_BTN_GAP: f32 = 6.0;

#[derive(Serialize, Deserialize)]
struct SavedWindow {
    file: String,
    view_kind: Option<SavedViewKind>,
    position: (f32, f32),
    size: (f32, f32),
    scroll: f32,
    #[serde(default)]
    scroll_x: f32,
    #[serde(default)]
    link_from: Option<SavedCallOrigin>,
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

#[derive(Serialize, Deserialize)]
struct SavedCallOrigin {
    file: String,
    line: usize,
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
    zoom: f32,
    minimap_dragging: bool,
    last_mouse_world: Option<Vector2>,
    last_click_time: Option<Instant>,
    last_click_pos: Option<Vector2>,
    last_click_window: Option<usize>,
    call_links: Vec<CallLink>,
    last_link_cycle: Option<(Vector2, usize)>, // click pos, index in group
}

struct MinimapContext {
    rect: Rectangle,
    bounds: Rectangle,
    scale: f32,
    origin: Vector2,
}

impl AppState {
    fn single_fn_title(&self, file: &PathBuf, start: usize) -> Option<String> {
        let pf = self.project.parsed.get(file)?;
        let func_name = pf
            .defs
            .iter()
            .find(|d| d.line == start)
            .or_else(|| pf.defs.iter().find(|d| d.line > start))
            .map(|d| d.name.clone())?;
        Some(func_name)
    }

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
            zoom: 1.0,
            minimap_dragging: false,
            last_mouse_world: None,
            last_click_time: None,
            last_click_pos: None,
            last_click_window: None,
            call_links: Vec::new(),
            last_link_cycle: None,
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
                        let vk = CodeViewKind::SingleFn { start, end };
                        title = self.single_fn_title(&file, start).unwrap_or(t);
                        vk
                    }
                    _ => CodeViewKind::FullFile,
                };
                let mut win = CodeWindow {
                    file,
                    title,
                    focus_line: None,
                    view_kind,
                    def_refs: Vec::new(),
                    call_refs: Vec::new(),
                    link_from: saved.link_from.and_then(|orig| {
                        Some(CallOrigin {
                            file: self.project.root.join(orig.file),
                            line: orig.line,
                        })
                    }),
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
                self.refresh_window_metadata(&mut win);
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
                link_from: w.link_from.as_ref().map(|o| SavedCallOrigin {
                    file: self.project.display_name(&o.file),
                    line: o.line,
                }),
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
            self.refresh_window_metadata(&mut win);
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
            file: path.clone(),
            title: path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file")
                .to_string(),
            focus_line: jump_to,
            view_kind: CodeViewKind::FullFile,
            def_refs: Vec::new(),
            call_refs: Vec::new(),
            link_from: None,
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
        self.refresh_window_metadata(&mut win);
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
        ctrl_down: bool,
        space_down: bool,
        screen_w: f32,
        screen_h: f32,
    ) {
        let mut world_mouse = Vector2::new(mouse.x / self.zoom - self.pan.x, mouse.y / self.zoom - self.pan.y);
        self.last_mouse_world = Some(world_mouse);

        let mut double_open: Option<(PathBuf, usize, Option<CallOrigin>)> = None;
        if left_pressed {
            let now = Instant::now();
            let hit_idx = self
                .windows
                .iter()
                .rposition(|w| w.hit_test(world_mouse));
            if let Some(idx) = hit_idx {
                let same_win = self.last_click_window == Some(idx);
                let close_pos = self
                    .last_click_pos
                    .map(|p| {
                        let dx = p.x - world_mouse.x;
                        let dy = p.y - world_mouse.y;
                        dx * dx + dy * dy < 16.0
                    })
                    .unwrap_or(false);
                let close_time = self
                    .last_click_time
                    .map(|t| now.duration_since(t) <= Duration::from_millis(350))
                    .unwrap_or(false);
                if same_win && close_pos && close_time {
                    if let CodeViewKind::SingleFn { start, .. } = self.windows[idx].view_kind {
                        let file = self.windows[idx].file.clone();
                        let origin = self.windows[idx].link_from.clone();
                        double_open = Some((file, start, origin));
                    }
                }
            }
            self.last_click_time = Some(now);
            self.last_click_pos = Some(world_mouse);
            self.last_click_window = hit_idx;
        }

        if let Some((file, line, origin)) = double_open {
            let title = file
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file")
                .to_string();
            self.open_file_with_view(file, Some(line), title, CodeViewKind::FullFile, origin);
            return;
        }

        if ctrl_down && wheel.abs() > f32::EPSILON {
            let factor = 1.0 + wheel * 0.1;
            let new_zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
            if (new_zoom - self.zoom).abs() > f32::EPSILON {
                // Keep the point under the cursor fixed while zooming; consider the sidebar offset.
                let world_anchor = Vector2::new(
                    (mouse.x - SIDEBAR_WIDTH) / self.zoom - self.pan.x,
                    mouse.y / self.zoom - self.pan.y,
                );
                self.zoom = new_zoom;
                self.pan = Vector2::new(
                    (mouse.x - SIDEBAR_WIDTH) / self.zoom - world_anchor.x,
                    mouse.y / self.zoom - world_anchor.y,
                );
                world_mouse =
                    Vector2::new(mouse.x / self.zoom - self.pan.x, mouse.y / self.zoom - self.pan.y);
                self.last_mouse_world = Some(world_mouse);
            }
            return;
        }

        if left_pressed {
            if let Some((link_idx, group_len)) = self.hit_call_link(world_mouse) {
                if let Some(link) = self.call_links.get(link_idx) {
                    let caller_idx = link.caller_idx;
                    if caller_idx < self.windows.len() {
                        let mut scrolled = false;
                        {
                            let win = &mut self.windows[caller_idx];
                            if self.project.parsed.contains_key(&win.file) {
                                let line = link.line;
                                let local_line = match win.view_kind {
                                    CodeViewKind::SingleFn { start, .. } => {
                                        line.saturating_sub(start)
                                    }
                                    _ => line,
                                };
                                let max_scroll = code_window::metrics_for(&self.project, win)
                                    .map(|m| m.max_scroll_y())
                                    .unwrap_or(0.0);
                                win.scroll = (local_line as f32 * LINE_HEIGHT - 40.0)
                                    .clamp(0.0, max_scroll);
                                win.focus_line = Some(line);
                                scrolled = true;
                            }
                        }
                        if scrolled {
                            self.bring_to_front(caller_idx);
                            self.last_link_cycle = Some((world_mouse, (link_idx + 1) % group_len));
                            return;
                        }
                    }
                }
            }
        }

        let pan_initiated = middle_pressed || (space_down && left_pressed);
        if pan_initiated {
            self.pan_dragging = true;
            self.pan_anchor = mouse;
            self.pan_start = self.pan;
        }
        let pan_active = middle_down || (space_down && left_down);
        if !pan_active {
            self.pan_dragging = false;
        }

        if self.pan_dragging {
            let dx = (mouse.x - self.pan_anchor.x) / self.zoom;
            let dy = (mouse.y - self.pan_anchor.y) / self.zoom;
            self.pan = Vector2::new(self.pan_start.x + dx, self.pan_start.y + dy);
            return;
        }

        let minimap_ctx = self.minimap_context(screen_w, screen_h);
        if let Some(ctx) = minimap_ctx {
            let buttons = self.minimap_buttons(&ctx);
            if left_pressed {
                if point_in_rect(mouse, buttons.0) {
                    if let Some(win) = self.windows.last() {
                        self.zoom_to_rect(win.rect_at(Vector2::new(0.0, 0.0)), screen_w, screen_h);
                    }
                    return;
                }
                if point_in_rect(mouse, buttons.1) {
                    if let Some(bounds) = self.world_bounds() {
                        self.zoom_to_rect(bounds, screen_w, screen_h);
                    }
                    return;
                }
                if point_in_rect(mouse, buttons.2) {
                    self.zoom = 1.0;
                    self.pan = Vector2::new(0.0, 0.0);
                    return;
                }
                if point_in_rect(mouse, ctx.rect) {
                    self.minimap_dragging = true;
                    let target = self.minimap_to_world(mouse, &ctx);
                    self.center_view_on(target, screen_w, screen_h);
                    return;
                }
            }
            if self.minimap_dragging && left_down {
                let target = self.minimap_to_world(mouse, &ctx);
                self.center_view_on(target, screen_w, screen_h);
                return;
            }
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
            self.minimap_dragging = false;
        }

        if wheel.abs() > f32::EPSILON {
            if self
                .sidebar
                .handle_wheel(mouse, wheel, &self.project, &self.project.defs, screen_h)
            {
                return;
            }
            for idx in (0..self.windows.len()).rev() {
                let content = self.windows[idx].content_rect_at(Vector2::new(0.0, 0.0));
                if point_in_rect(world_mouse, content) {
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
                    w.hover_edges = w.hit_resize_edges(world_mouse, Vector2::new(0.0, 0.0));
                }
            }
        }

        if left_pressed {
            let mut handled = false;
            for idx in (0..self.windows.len()).rev() {
                if self.windows[idx].hit_test(world_mouse) {
                    self.bring_to_front(idx);
                    let top_idx = self.windows.len() - 1;
                    let action = self.handle_window_click(font, top_idx, world_mouse);
                    match action {
                        WindowAction::Close => {
                            self.windows.pop();
                        }
                        WindowAction::OpenDefinition { def, origin } => {
                            self.open_definition(def, origin);
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

    fn world_bounds(&self) -> Option<Rectangle> {
        if self.windows.is_empty() {
            return None;
        }
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for w in &self.windows {
            min_x = min_x.min(w.position.x);
            min_y = min_y.min(w.position.y);
            max_x = max_x.max(w.position.x + w.size.x);
            max_y = max_y.max(w.position.y + w.size.y);
        }
        if min_x.is_infinite() || min_y.is_infinite() {
            return None;
        }
        let padding = 24.0;
        Some(Rectangle {
            x: min_x - padding,
            y: min_y - padding,
            width: (max_x - min_x) + padding * 2.0,
            height: (max_y - min_y) + padding * 2.0,
        })
    }

    fn minimap_context(&self, screen_w: f32, _screen_h: f32) -> Option<MinimapContext> {
        let bounds = self.world_bounds()?;
        let rect = Rectangle {
            x: (screen_w - MINIMAP_W - MINIMAP_MARGIN).max(0.0),
            y: MINIMAP_MARGIN,
            width: MINIMAP_W,
            height: MINIMAP_H,
        };
        let button_row = MINIMAP_BTN_H + MINIMAP_BTN_GAP + 2.0;
        let avail_w = (rect.width - MINIMAP_PAD * 2.0).max(1.0);
        let avail_h = (rect.height - MINIMAP_PAD * 2.0 - button_row).max(1.0);
        let scale = (avail_w / bounds.width)
            .min(avail_h / bounds.height)
            .max(0.01);
        let origin = Vector2 {
            x: rect.x + MINIMAP_PAD - bounds.x * scale,
            y: rect.y + MINIMAP_PAD - bounds.y * scale,
        };
        Some(MinimapContext {
            rect,
            bounds,
            scale,
            origin,
        })
    }

    fn minimap_buttons(&self, ctx: &MinimapContext) -> (Rectangle, Rectangle, Rectangle) {
        let y = ctx.rect.y + ctx.rect.height - MINIMAP_BTN_H - MINIMAP_PAD;
        let x0 = ctx.rect.x + MINIMAP_PAD;
        let b0 = Rectangle {
            x: x0,
            y,
            width: MINIMAP_BTN_W,
            height: MINIMAP_BTN_H,
        };
        let b1 = Rectangle {
            x: x0 + MINIMAP_BTN_W + MINIMAP_BTN_GAP,
            y,
            width: MINIMAP_BTN_W,
            height: MINIMAP_BTN_H,
        };
        let b2 = Rectangle {
            x: x0 + (MINIMAP_BTN_W + MINIMAP_BTN_GAP) * 2.0,
            y,
            width: MINIMAP_BTN_W,
            height: MINIMAP_BTN_H,
        };
        (b0, b1, b2)
    }

    fn minimap_to_world(&self, mouse: Vector2, ctx: &MinimapContext) -> Vector2 {
        Vector2 {
            x: (mouse.x - ctx.origin.x) / ctx.scale,
            y: (mouse.y - ctx.origin.y) / ctx.scale,
        }
    }

    fn center_view_on(&mut self, world: Vector2, screen_w: f32, screen_h: f32) {
        self.pan = Vector2::new(
            screen_w / (2.0 * self.zoom) - world.x,
            screen_h / (2.0 * self.zoom) - world.y,
        );
    }

    fn zoom_to_rect(&mut self, rect: Rectangle, screen_w: f32, screen_h: f32) {
        let target_zoom = (screen_w / rect.width).min(screen_h / rect.height) * 0.9;
        self.zoom = target_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let center = Vector2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        self.center_view_on(center, screen_w, screen_h);
    }

    fn draw_minimap(&self, d: &mut RaylibDrawHandle, ctx: &MinimapContext) {
        d.draw_rectangle(
            ctx.rect.x as i32,
            ctx.rect.y as i32,
            ctx.rect.width as i32,
            ctx.rect.height as i32,
            self.palette.window,
        );
        d.draw_rectangle_lines(
            ctx.rect.x as i32,
            ctx.rect.y as i32,
            ctx.rect.width as i32,
            ctx.rect.height as i32,
            self.palette.title,
        );

        for (idx, win) in self.windows.iter().enumerate() {
            let x = ctx.origin.x + (win.position.x - ctx.bounds.x) * ctx.scale;
            let y = ctx.origin.y + (win.position.y - ctx.bounds.y) * ctx.scale;
            let w = win.size.x * ctx.scale;
            let h = win.size.y * ctx.scale;
            let rect = Rectangle {
                x,
                y,
                width: w,
                height: h,
            };
            let color = if idx + 1 == self.windows.len() {
                self.palette.window_top
            } else {
                self.palette.window
            };
            d.draw_rectangle_rec(rect, color);
            d.draw_rectangle_lines(
                rect.x as i32,
                rect.y as i32,
                rect.width as i32,
                rect.height as i32,
                self.palette.sidebar_highlight,
            );
        }

        let canvas_w = (d.get_screen_width() as f32 - SIDEBAR_WIDTH).max(0.0);
        let view_rect = Rectangle {
            x: -self.pan.x + SIDEBAR_WIDTH / self.zoom,
            y: -self.pan.y,
            width: canvas_w / self.zoom,
            height: d.get_screen_height() as f32 / self.zoom,
        };
        let vx = ctx.origin.x + (view_rect.x - ctx.bounds.x) * ctx.scale;
        let vy = ctx.origin.y + (view_rect.y - ctx.bounds.y) * ctx.scale;
        let vw = view_rect.width * ctx.scale;
        let vh = view_rect.height * ctx.scale;
        d.draw_rectangle_lines(
            vx as i32,
            vy as i32,
            vw as i32,
            vh as i32,
            self.palette.close,
        );

        let buttons = self.minimap_buttons(ctx);
        let labels = ["Current", "Fit All", "Reset"];
        let btns = [buttons.0, buttons.1, buttons.2];
        for (i, rect) in btns.iter().enumerate() {
            d.draw_rectangle(
                rect.x as i32,
                rect.y as i32,
                rect.width as i32,
                rect.height as i32,
                self.palette.title,
            );
            let text = labels[i];
            d.draw_text(
                text,
                (rect.x + 6.0) as i32,
                (rect.y + 3.0) as i32,
                10,
                self.palette.text,
            );
        }
    }

    pub fn draw(&mut self, d: &mut RaylibDrawHandle, font: &AppFont, mouse: Vector2) {
        let mut hover_cursor: Option<MouseCursor> = if self.pan_dragging {
            Some(MouseCursor::MOUSE_CURSOR_RESIZE_ALL)
        } else {
            None
        };
        // draw background 20% darker
        let dark_bg = Color {
            r: (self.palette.bg.r as f32 * 0.8) as u8,
            g: (self.palette.bg.g as f32 * 0.8) as u8,
            b: (self.palette.bg.b as f32 * 0.8) as u8,
            a: self.palette.bg.a,
        };
        d.clear_background(dark_bg);
        let world_mouse = Vector2::new(
            mouse.x / self.zoom - self.pan.x,
            mouse.y / self.zoom - self.pan.y,
        );
        let camera = Camera2D {
            offset: Vector2::new(0.0, 0.0),
            target: Vector2::new(-self.pan.x, -self.pan.y),
            rotation: 0.0,
            zoom: self.zoom,
        };
        {
            let mut scoped = d.begin_mode2D(camera);
            self.draw_call_links(&mut scoped, world_mouse);
            for idx in 0..self.windows.len() {
                if hover_cursor.is_none() {
                    if let Some(edges) = self.windows[idx].hover_edges {
                        hover_cursor = Some(cursor_for_edges(edges));
                    }
                }
                if hover_cursor.is_none() {
                    if self
                        .call_links
                        .iter()
                        .any(|l| l.hovered && min_distance_to_cubic(&l.points, world_mouse) <= 10.0)
                    {
                        hover_cursor = Some(MouseCursor::MOUSE_CURSOR_POINTING_HAND);
                    } else if let Some(pf) = self.project.parsed.get(&self.windows[idx].file) {
                        if code_window::is_over_call(
                            font,
                            pf,
                            &self.windows[idx],
                            world_mouse,
                            &self.project,
                        ) {
                            hover_cursor = Some(MouseCursor::MOUSE_CURSOR_POINTING_HAND);
                        }
                    }
                }
                let is_top = idx + 1 == self.windows.len();
                self.windows[idx].draw_window(
                    &mut scoped,
                    font,
                    &self.palette,
                    &self.icons,
                    &self.project,
                    is_top,
                    self.pan,
                    self.zoom,
                );
            }
        }
        d.set_mouse_cursor(hover_cursor.unwrap_or(MouseCursor::MOUSE_CURSOR_DEFAULT));
        if let Some(ctx) =
            self.minimap_context(d.get_screen_width() as f32, d.get_screen_height() as f32)
        {
            self.draw_minimap(d, &ctx);
        }
        self.sidebar.draw(
            d,
            font,
            mouse,
            &self.project,
            &self.project.defs,
            &self.palette,
        );
    }

    fn refresh_window_metadata(&self, win: &mut CodeWindow) {
        if let Some(pf) = self.project.parsed.get(&win.file) {
            win.update_refs(pf);
        } else {
            win.def_refs.clear();
            win.call_refs.clear();
        }
    }

    fn draw_call_links(&mut self, d: &mut RaylibMode2D<RaylibDrawHandle>, world_mouse: Vector2) {
        self.call_links.clear();
        let mut fn_windows = Vec::new();
        for (idx, win) in self.windows.iter().enumerate() {
            if let CodeViewKind::SingleFn { start, .. } = win.view_kind {
                if let Some(def) = win.def_refs.iter().find(|d| d.line >= start) {
                    fn_windows.push((idx, def.name.clone(), def.module_path.clone(), def.line));
                }
            }
        }

        let mut highlights = Vec::new();
        for (fn_idx, fn_name, fn_module, _fn_start) in fn_windows {
            let fn_win = &self.windows[fn_idx];

            let mut callers: Vec<(usize, usize)> = Vec::new();
            if let Some(origin) = &fn_win.link_from {
                if let Some(idx) = self.windows.iter().position(|w| w.file == origin.file) {
                    if let Some(caller) = self.windows.get(idx) {
                        let strict_match = caller.call_refs.iter().any(|c| {
                            c.line == origin.line && c.name == fn_name && c.module_path == fn_module
                        });
                        let loose_match = caller
                            .call_refs
                            .iter()
                            .any(|c| c.line == origin.line && c.name == fn_name);
                        if strict_match || loose_match {
                            callers.push((idx, origin.line));
                        }
                    }
                }
            }

            for (idx, caller_win) in self.windows.iter().enumerate() {
                if idx == fn_idx {
                    continue;
                }
                let mut matches: Vec<&code_window::CallRef> = caller_win
                    .call_refs
                    .iter()
                    .filter(|c| c.name == fn_name && c.module_path == fn_module)
                    .collect();
                if matches.is_empty() {
                    matches = caller_win
                        .call_refs
                        .iter()
                        .filter(|c| c.name == fn_name)
                        .collect();
                }
                if matches.is_empty() {
                    continue;
                }
                for call in matches {
                    callers.push((idx, call.line));
                }
            }

            callers.sort_by_key(|(idx, line)| (*idx, *line));
            callers.dedup();

            for (caller_idx, line) in callers {
                let caller_win = &self.windows[caller_idx];
                let caller_pf = match self.project.parsed.get(&caller_win.file) {
                    Some(pf) => pf,
                    None => continue,
                };
                let start_pt = caller_win
                    .line_anchor(caller_pf, line, true)
                    .unwrap_or_else(|| caller_win.center_anchor(true));
                let end_pt = fn_win.center_anchor(false);
                let dx = (end_pt.x - start_pt.x).abs().max(40.0);
                let dir = if end_pt.x >= start_pt.x { 1.0 } else { -1.0 };
                let handle = dx * 0.35;
                let c1 = Vector2::new(start_pt.x + dir * handle, start_pt.y);
                let c2 = Vector2::new(end_pt.x - dir * handle, end_pt.y);
                let points = [start_pt, c1, c2, end_pt];
                let hovered = min_distance_to_cubic(&points, world_mouse) <= 10.0;
                self.call_links.push(CallLink {
                    points,
                    caller_idx,
                    line,
                    hovered,
                    target_idx: fn_idx,
                });
                if let Some(rect) = caller_win.call_highlight_rect(caller_pf, line, true) {
                    highlights.push((rect, caller_idx));
                }
            }
        }
        let active_idx = self.windows.len().saturating_sub(1);
        let active_is_fn = self
            .windows
            .get(active_idx)
            .map(|w| matches!(w.view_kind, CodeViewKind::SingleFn { .. }))
            .unwrap_or(false);
        for link in &self.call_links {
            let active_link = active_is_fn && link.target_idx == active_idx;
            let color = if link.hovered {
                self.palette.close
            } else if active_link {
                self.palette.title
            } else {
                self.palette.sidebar_highlight
            };
            d.draw_spline_bezier_cubic(&link.points, 2.5, color);
        }

        for (rect, _idx) in highlights {
            d.draw_rectangle_lines_ex(rect, 1.0, self.palette.sidebar_highlight);
        }
    }

    fn handle_window_click(
        &mut self,
        font: &AppFont,
        idx: usize,
        world_mouse: Vector2,
    ) -> WindowAction {
        let win = &self.windows[idx];
        let title_rect = win.title_rect_at(Vector2::new(0.0, 0.0));
        let icon_size = self.icons.size() as f32;
        let close_rect = Rectangle {
            x: title_rect.x + title_rect.width - icon_size - 8.0,
            y: title_rect.y + (TITLE_BAR_HEIGHT - icon_size) * 0.5,
            width: icon_size,
            height: icon_size,
        };

        if point_in_rect(world_mouse, close_rect) {
            return WindowAction::Close;
        }

        if let Some(edges) = win.hit_resize_edges(world_mouse, Vector2::new(0.0, 0.0)) {
            return WindowAction::StartResize { edges };
        }

        if point_in_rect(world_mouse, title_rect) {
            return WindowAction::StartDrag(Vector2 {
                x: world_mouse.x - win.position.x,
                y: world_mouse.y - win.position.y,
            });
        }

        let content_rect = win.content_rect_at(Vector2::new(0.0, 0.0));
        if !point_in_rect(world_mouse, content_rect) {
            return WindowAction::None;
        }

        if let Some(pf) = self.project.parsed.get(&win.file) {
            let metrics = code_window::content_metrics(pf, win);
            if let Some(mini) = win.minimap_rect_at(&metrics, Vector2::new(0.0, 0.0)) {
                if point_in_rect(world_mouse, mini) {
                    let ratio = ((world_mouse.y - mini.y) / mini.height).clamp(0.0, 1.0);
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
                    let ratio = ((world_mouse.y - track_y - thumb_h * 0.5) / denom).clamp(0.0, 1.0);
                    let grab_offset = (world_mouse.y - thumb_y).clamp(0.0, thumb_h);
                    let v_rect = Rectangle {
                        x: track_x,
                        y: track_y,
                        width: SCROLLBAR_THICKNESS,
                        height: track_h,
                    };
                    if point_in_rect(world_mouse, v_rect) {
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
                    let ratio = ((world_mouse.x - track_x - thumb_w * 0.5) / denom).clamp(0.0, 1.0);
                    let grab_offset = (world_mouse.x - thumb_x).clamp(0.0, thumb_w);
                    let h_rect = Rectangle {
                        x: track_x,
                        y: track_y,
                        width: track_w,
                        height: SCROLLBAR_THICKNESS,
                    };
                    if point_in_rect(world_mouse, h_rect) {
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
                if point_in_rect(world_mouse, v_rect) {
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
                if point_in_rect(world_mouse, h_rect) {
                    return WindowAction::None;
                }
            }
            if let Some((def, origin)) =
                code_window::hit_test_calls(font, pf, win, world_mouse, &self.project)
            {
                return WindowAction::OpenDefinition {
                    def,
                    origin: Some(origin),
                };
            }
        }

        WindowAction::None
    }

    fn bring_to_front(&mut self, idx: usize) {
        if idx + 1 == self.windows.len() {
            return;
        }
        let w = self.windows.remove(idx);
        self.windows.push(w);
    }

    fn open_definition(&mut self, def: DefinitionLocation, origin: Option<CallOrigin>) {
        let file_name = def
            .file
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("fn")
            .to_string();

        let (title, view_kind) = if let Some(pf) = self.project.parsed.get(&def.file) {
            if let Some((start, end)) = find_function_span(pf, def.line) {
                let title = self
                    .single_fn_title(&def.file, start)
                    .unwrap_or_else(|| file_name.clone());
                (title, CodeViewKind::SingleFn { start, end })
            } else {
                (
                    def.module_path
                        .split("::")
                        .last()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| file_name.clone()),
                    CodeViewKind::FullFile,
                )
            }
        } else {
            (
                def.module_path
                    .split("::")
                    .last()
                    .map(|s| s.to_string())
                    .unwrap_or(file_name.clone()),
                CodeViewKind::FullFile,
            )
        };

        self.open_file_with_view(def.file, Some(def.line), title, view_kind, origin);
    }

    fn open_file_with_view(
        &mut self,
        path: PathBuf,
        jump_to: Option<usize>,
        title: String,
        view_kind: CodeViewKind,
        origin: Option<CallOrigin>,
    ) {
        if let Some(idx) = self
            .windows
            .iter()
            .position(|w| w.file == path && matches_view(&w.view_kind, &view_kind))
        {
            let mut win = self.windows.remove(idx);
            win.title = title.clone();
            win.link_from = origin.clone().or(win.link_from);
            if let Some(line) = jump_to {
                let local_line = match view_kind {
                    CodeViewKind::SingleFn { start, .. } => line.saturating_sub(start),
                    _ => line,
                };
                win.scroll = (local_line as f32 * LINE_HEIGHT - 40.0).max(0.0);
                win.focus_line = Some(line);
            }
            code_window::clamp_window_scroll(&self.project, &mut win);
            self.refresh_window_metadata(&mut win);
            self.windows.push(win);
            return;
        }

        let pos = Vector2::new(
            SIDEBAR_WIDTH + 24.0 + (self.windows.len() as f32 * 18.0),
            40.0 + (self.windows.len() as f32 * 18.0),
        );
        let mut scroll = 0.0;
        if let Some(line) = jump_to {
            let local_line = match view_kind {
                CodeViewKind::SingleFn { start, .. } => line.saturating_sub(start),
                _ => line,
            };
            scroll = (local_line as f32 * LINE_HEIGHT - 40.0).max(0.0);
        }
        let win_size = Vector2::new(720.0, 460.0);
        let mut win = CodeWindow {
            file: path.clone(),
            title,
            focus_line: jump_to,
            view_kind,
            def_refs: Vec::new(),
            call_refs: Vec::new(),
            link_from: origin,
            position: self
                .last_mouse_world
                .map(|m| Vector2::new(m.x - win_size.x * 0.5, m.y - TITLE_BAR_HEIGHT * 0.5))
                .unwrap_or(pos),
            size: win_size,
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
        self.refresh_window_metadata(&mut win);
        self.next_window_id += 1;
        self.windows.push(win);
    }
}

impl AppState {
    fn hit_call_link(&mut self, world_mouse: Vector2) -> Option<(usize, usize)> {
        let tolerance = 10.0;
        let mut hits: Vec<usize> = self
            .call_links
            .iter()
            .enumerate()
            .filter_map(|(i, link)| {
                let dist = min_distance_to_cubic(&link.points, world_mouse);
                if dist <= tolerance {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        if hits.is_empty() {
            return None;
        }
        hits.sort();
        let mut chosen = hits[0];
        // Decide cycling direction based on mouse position relative to the caller window.
        let mut dir: isize = 1;
        if let Some(link) = self.call_links.get(chosen) {
            if let Some(win) = self.windows.get(link.caller_idx) {
                let center_y = win.position.y + win.size.y * 0.5;
                dir = if world_mouse.y < center_y { -1 } else { 1 };
            }
        }
        if let Some((last_pos, last_idx)) = self.last_link_cycle {
            let dx = last_pos.x - world_mouse.x;
            let dy = last_pos.y - world_mouse.y;
            if dx * dx + dy * dy < tolerance * tolerance {
                if let Some(pos) = hits.iter().position(|i| *i == last_idx) {
                    let next_pos = if dir < 0 {
                        (pos + hits.len() - 1) % hits.len()
                    } else {
                        (pos + 1) % hits.len()
                    };
                    chosen = hits[next_pos];
                }
            }
        }
        Some((chosen, hits.len()))
    }
}

#[derive(Clone, Debug)]
enum WindowAction {
    None,
    Close,
    OpenDefinition {
        def: DefinitionLocation,
        origin: Option<CallOrigin>,
    },
    StartDrag(Vector2),
    StartResize {
        edges: (bool, bool, bool, bool),
    },
    StartVScroll {
        grab_offset: f32,
        ratio: f32,
    },
    StartHScroll {
        grab_offset: f32,
        ratio: f32,
    },
    StartMinimap {
        ratio: f32,
    },
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

fn min_distance_to_cubic(points: &[Vector2; 4], p: Vector2) -> f32 {
    let mut min_d2 = f32::MAX;
    let mut prev = points[0];
    let steps = 24;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let cur = cubic_point(points, t);
        let d2 = dist2_point_segment(p, prev, cur);
        if d2 < min_d2 {
            min_d2 = d2;
        }
        prev = cur;
    }
    min_d2.sqrt()
}

fn cubic_point(p: &[Vector2; 4], t: f32) -> Vector2 {
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let t2 = t * t;
    Vector2::new(
        mt2 * mt * p[0].x + 3.0 * mt2 * t * p[1].x + 3.0 * mt * t2 * p[2].x + t2 * t * p[3].x,
        mt2 * mt * p[0].y + 3.0 * mt2 * t * p[1].y + 3.0 * mt * t2 * p[2].y + t2 * t * p[3].y,
    )
}

fn dist2_point_segment(p: Vector2, a: Vector2, b: Vector2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let ab_len2 = ab.x * ab.x + ab.y * ab.y;
    if ab_len2 <= f32::EPSILON {
        return ap.x * ap.x + ap.y * ap.y;
    }
    let t = ((ap.x * ab.x + ap.y * ab.y) / ab_len2).clamp(0.0, 1.0);
    let proj = Vector2::new(a.x + ab.x * t, a.y + ab.y * t);
    let d = p - proj;
    d.x * d.x + d.y * d.y
}
