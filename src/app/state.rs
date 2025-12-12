use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;
use std::time::Instant;

use raylib::prelude::*;
use serde_json;

use crate::code_window::{self, CallOrigin, CodeViewKind, CodeWindow, FoldRegion};
use crate::constants::{
    LAYOUT_FILE, LINE_HEIGHT, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, TITLE_BAR_HEIGHT,
};
use crate::helpers::matches_view;
use crate::icons::Icons;
use crate::model::{DefinitionLocation, ProjectModel, find_function_span};
use crate::sidebar::{SidebarAction, SidebarState};
use crate::theme::Palette;

use super::types::{
    CallLink, DepMode, SavedCallOrigin, SavedLayout, SavedViewKind, SavedWindow, ThemeMode,
};

pub struct AppState {
    pub project: ProjectModel,
    pub windows: Vec<CodeWindow>,
    pub next_window_id: usize,
    pub sidebar: SidebarState,
    pub palette: Palette,
    pub app_palette: Palette,
    pub code_palette: Palette,
    pub theme_mode: ThemeMode,
    pub dep_mode: DepMode,
    pub deps_loaded: bool,
    pub(crate) icons: Icons,
    pub(crate) pan: Vector2,
    pub(crate) pan_dragging: bool,
    pub(crate) pan_anchor: Vector2,
    pub(crate) pan_start: Vector2,
    pub(crate) zoom: f32,
    pub(crate) minimap_dragging: bool,
    pub(crate) last_mouse_world: Option<Vector2>,
    pub(crate) last_click_time: Option<Instant>,
    pub(crate) last_click_pos: Option<Vector2>,
    pub(crate) last_click_window: Option<usize>,
    pub(crate) call_links: Vec<CallLink>,
    pub(crate) last_link_cycle: Option<(Vector2, usize)>, // click pos, index in group
    pub(crate) project_dirty: bool,
    pub(crate) last_reload: Instant,
    pub(crate) reload_rx: Option<Receiver<anyhow::Result<ProjectModel>>>,
    pub(crate) reload_inflight: bool,
    pub(crate) pending_dep_reload: bool,
    pub(crate) pending_project_root: Option<PathBuf>,
    pub(crate) sidebar_resizing: bool,
    pub(crate) sidebar_resize_anchor: f32,
    pub(crate) sidebar_resize_start: f32,
}

impl AppState {
    pub fn new(
        project: ProjectModel,
        rl: &mut RaylibHandle,
        thread: &RaylibThread,
        app_palette: Palette,
        code_palette: Palette,
        theme_mode: ThemeMode,
        dep_mode: DepMode,
        deps_loaded: bool,
    ) -> Self {
        let icons = Icons::load(rl, thread, 16);
        let palette = match theme_mode {
            ThemeMode::Application => app_palette,
            ThemeMode::Code => code_palette,
        };
        let mut state = Self {
            project,
            windows: Vec::new(),
            next_window_id: 1,
            sidebar: SidebarState::with_icons(rl, thread),
            palette,
            app_palette,
            code_palette,
            theme_mode,
            dep_mode,
            deps_loaded,
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
            project_dirty: false,
            last_reload: Instant::now(),
            reload_rx: None,
            reload_inflight: false,
            pending_dep_reload: false,
            pending_project_root: None,
            sidebar_resizing: false,
            sidebar_resize_anchor: 0.0,
            sidebar_resize_start: 0.0,
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

    pub fn sidebar_width(&self) -> f32 {
        self.sidebar.current_width()
    }

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
            if let Some(width) = layout.sidebar_width {
                self.sidebar.width = width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
            }
            self.sidebar.collapsed = layout.sidebar_hidden;
            if let Some(mode) = layout.theme_mode {
                self.theme_mode = mode;
                self.apply_theme_mode();
            }
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
                    folds: Vec::new(),
                    fold_version: 0,
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
                    metrics_cache: RefCell::new(None),
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
            sidebar_width: Some(self.sidebar.width),
            sidebar_hidden: self.sidebar.collapsed,
            theme_mode: Some(self.theme_mode),
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
            self.sidebar_width() + 24.0 + (self.windows.len() as f32 * 18.0),
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
            folds: Vec::new(),
            fold_version: 0,
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
            metrics_cache: RefCell::new(None),
        };
        self.seed_folds_from_peers(&mut win);
        code_window::clamp_window_scroll(&self.project, &mut win);
        self.refresh_window_metadata(&mut win);
        self.next_window_id += 1;
        self.windows.push(win);
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
        for pf in self.project.parsed.values() {
            pf.color_cache.borrow_mut().take();
        }
    }

    pub fn apply_theme_mode(&mut self) {
        let palette = match self.theme_mode {
            ThemeMode::Application => self.app_palette,
            ThemeMode::Code => self.code_palette,
        };
        self.set_palette(palette);
    }

    pub fn warm_load_deps(&mut self) {
        if matches!(self.dep_mode, DepMode::Lazy) && !self.deps_loaded {
            self.spawn_reload(true);
        }
    }

    fn spawn_reload(&mut self, include_deps: bool) {
        if self.reload_inflight {
            return;
        }
        let root = self.project.root.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.reload_rx = Some(rx);
        self.reload_inflight = true;
        self.pending_dep_reload = include_deps;
        std::thread::spawn(move || {
            let res = ProjectModel::load(&root, include_deps);
            let _ = tx.send(res);
        });
    }

    pub fn handle_sidebar_action(&mut self, action: SidebarAction) {
        match action {
            SidebarAction::OpenFile { path, line } => self.open_file(path, line),
            SidebarAction::ToggleDir => {}
            SidebarAction::ToggleCollapse => {
                self.sidebar.collapsed = !self.sidebar.collapsed;
                self.sidebar_resizing = false;
                self.sidebar.search_focused = false;
                self.sidebar.width = self
                    .sidebar
                    .width
                    .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
            }
            SidebarAction::ToggleTheme => {
                self.theme_mode = self.theme_mode.toggle();
                self.apply_theme_mode();
            }
            SidebarAction::OpenFolder => {
                if let Some(path) = pick_project_root(&self.project.root) {
                    self.pending_project_root = Some(path);
                }
            }
        }
    }

    pub fn open_definition(&mut self, def: DefinitionLocation, origin: Option<CallOrigin>) {
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

    pub(crate) fn open_file_with_view(
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
            self.sidebar_width() + 24.0 + (self.windows.len() as f32 * 18.0),
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
            folds: Vec::new(),
            fold_version: 0,
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
            metrics_cache: RefCell::new(None),
        };
        self.seed_folds_from_peers(&mut win);
        code_window::clamp_window_scroll(&self.project, &mut win);
        self.refresh_window_metadata(&mut win);
        self.next_window_id += 1;
        self.windows.push(win);
    }

    pub(crate) fn refresh_window_metadata(&self, win: &mut CodeWindow) {
        Self::refresh_window_metadata_with_project(&self.project, win);
    }

    pub(crate) fn refresh_window_metadata_with_project(
        project: &ProjectModel,
        win: &mut CodeWindow,
    ) {
        if let Some(pf) = project.parsed.get(&win.file) {
            win.update_refs(pf);
            win.clear_metrics_cache();
        } else {
            win.def_refs.clear();
            win.call_refs.clear();
            win.clear_metrics_cache();
        }
    }

    fn seed_folds_from_peers(&self, win: &mut CodeWindow) {
        let Some(pf) = self.project.parsed.get(&win.file) else {
            return;
        };
        let (view_start, view_end) = win.view_range(pf);
        for peer in self.windows.iter().filter(|w| w.file == win.file) {
            for fold in &peer.folds {
                if !CodeWindow::fold_has_body(fold) {
                    continue;
                }
                if fold.start < view_start || fold.start > view_end {
                    continue;
                }
                if win.folds.iter().any(|f| f.start == fold.start) {
                    continue;
                }
                win.folds.push(fold.clone());
            }
        }
        if !win.folds.is_empty() {
            win.fold_version = win.fold_version.wrapping_add(1);
        }
    }

    pub(crate) fn sync_folds_to_siblings(
        &mut self,
        source_idx: usize,
        file: &PathBuf,
        folds: &[FoldRegion],
    ) {
        let Some(pf) = self.project.parsed.get(file) else {
            return;
        };
        for (idx, win) in self.windows.iter_mut().enumerate() {
            if idx == source_idx || win.file != *file {
                continue;
            }
            let (view_start, view_end) = win.view_range(pf);
            let mut changed = false;
            for fold in folds {
                if !CodeWindow::fold_has_body(fold) {
                    continue;
                }
                if fold.start < view_start || fold.start > view_end {
                    continue;
                }
                if let Some(existing) = win.folds.iter_mut().find(|f| f.start == fold.start) {
                    if existing.end != fold.end || existing.collapsed != fold.collapsed {
                        *existing = fold.clone();
                        changed = true;
                    }
                } else {
                    win.folds.push(fold.clone());
                    changed = true;
                }
            }
            if changed {
                win.fold_version = win.fold_version.wrapping_add(1);
                win.clear_metrics_cache();
                code_window::clamp_window_scroll(&self.project, win);
            }
        }
    }

    pub(crate) fn max_scroll_x(&self, win: &CodeWindow) -> Option<f32> {
        code_window::metrics_for(&self.project, win).map(|m| m.max_scroll_x())
    }

    pub(crate) fn max_scroll(&self, idx: usize) -> f32 {
        code_window::metrics_for(&self.project, &self.windows[idx])
            .map(|m| m.max_scroll_y())
            .unwrap_or(0.0)
    }

    pub(crate) fn bring_to_front(&mut self, idx: usize) {
        if idx + 1 == self.windows.len() {
            return;
        }
        let w = self.windows.remove(idx);
        self.windows.push(w);
    }

    pub(crate) fn world_bounds(&self) -> Option<Rectangle> {
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

    pub fn mark_project_dirty(&mut self) {
        self.project_dirty = true;
    }

    pub fn reload_project_if_needed(&mut self) -> anyhow::Result<()> {
        if self.reload_inflight {
            if let Some(rx) = self.reload_rx.as_ref() {
                if let Ok(res) = rx.try_recv() {
                    self.reload_inflight = false;
                    self.project_dirty = false;
                    self.last_reload = Instant::now();
                    self.reload_rx = None;
                    if self.pending_dep_reload {
                        self.deps_loaded = true;
                    }
                    self.pending_dep_reload = false;
                    let new_project = res?;
                    self.project = new_project;
                    self.sidebar.mark_tree_dirty();
                    {
                        let project_ref = &self.project;
                        for win in &mut self.windows {
                            code_window::clamp_window_scroll(project_ref, win);
                        }
                    }
                    for win in &mut self.windows {
                        Self::refresh_window_metadata_with_project(&self.project, win);
                    }
                }
            }
            return Ok(());
        }
        if !self.project_dirty {
            return Ok(());
        }
        if Instant::now().duration_since(self.last_reload) < Duration::from_millis(200) {
            return Ok(());
        }
        let include_deps = match self.dep_mode {
            DepMode::Off => false,
            DepMode::Lazy => self.deps_loaded,
            DepMode::Eager => true,
        };
        self.spawn_reload(include_deps);
        Ok(())
    }

    pub fn take_pending_project_root(&mut self) -> Option<PathBuf> {
        self.pending_project_root.take()
    }
}

fn pick_project_root(current: &PathBuf) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if current.exists() {
        dialog = dialog.set_directory(current);
    }
    dialog.pick_folder()
}
