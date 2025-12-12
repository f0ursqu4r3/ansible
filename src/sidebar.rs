use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use raylib::prelude::*;

use crate::constants::{FONT_SIZE, SIDEBAR_COLLAPSED_WIDTH, SIDEBAR_ROW_H, SIDEBAR_WIDTH};
use crate::icons::{Icon, Icons};
use crate::model::{DefinitionLocation, ProjectModel};
use crate::theme::Palette;
use crate::{AppFont, point_in_rect};

const SCROLLBAR_WIDTH: f32 = 6.0;
const SCROLLBAR_GUTTER: f32 = 10.0;
const SCROLLBAR_MIN_THUMB: f32 = 16.0;

#[derive(Clone)]
struct TreeEntry {
    path: PathBuf,
    display: String,
    depth: usize,
    is_dir: bool,
}

pub enum SidebarAction {
    OpenFile { path: PathBuf, line: Option<usize> },
    ToggleDir,
    ToggleCollapse,
    ToggleTheme,
    OpenFolder,
}

pub struct SidebarState {
    pub search_query: String,
    pub search_focused: bool,
    pub scroll: f32,
    pub collapsed_dirs: HashSet<PathBuf>,
    pub width: f32,
    pub collapsed: bool,
    icons: Icons,
    entries: Vec<TreeEntry>,
    entries_version: usize,
    entries_dirty: bool,
}

impl SidebarState {
    pub fn with_icons(rl: &mut RaylibHandle, thread: &RaylibThread) -> Self {
        Self {
            search_query: String::new(),
            search_focused: false,
            scroll: 0.0,
            collapsed_dirs: HashSet::new(),
            width: SIDEBAR_WIDTH,
            collapsed: false,
            icons: Icons::load(rl, thread, 16),
            entries: Vec::new(),
            entries_version: 0,
            entries_dirty: true,
        }
    }

    pub fn mark_tree_dirty(&mut self) {
        self.entries_dirty = true;
    }

    pub fn current_width(&self) -> f32 {
        if self.collapsed {
            SIDEBAR_COLLAPSED_WIDTH
        } else {
            self.width
        }
    }

    pub fn search_rect(&self) -> Rectangle {
        let w = self.current_width();
        Rectangle {
            x: 12.0,
            y: 40.0,
            width: w - 24.0,
            height: 26.0,
        }
    }

    fn list_start_y(&self) -> f32 {
        let search_rect = self.search_rect();
        search_rect.y + search_rect.height + 10.0
    }

    fn is_collapsed<P: AsRef<Path>>(&self, path: P) -> bool {
        self.collapsed_dirs.contains(path.as_ref())
    }

    fn toggle_dir<P: AsRef<Path>>(&mut self, path: P) {
        let p = path.as_ref();
        if self.collapsed_dirs.contains(p) {
            self.collapsed_dirs.remove(p);
        } else {
            self.collapsed_dirs.insert(p.to_path_buf());
        }
    }

    fn entry_rect(&self, entry: &TreeEntry, y: f32) -> Rectangle {
        let w = self.current_width();
        Rectangle {
            x: 10.0 + (entry.depth as f32 * 14.0),
            y,
            width: w - 20.0 - (entry.depth as f32 * 14.0),
            height: SIDEBAR_ROW_H,
        }
    }

    fn toggle_rect(&self) -> Rectangle {
        let w = self.current_width();
        Rectangle {
            x: (w - 24.0).max(2.0),
            y: 8.0,
            width: 16.0,
            height: 16.0,
        }
    }

    fn theme_rect(&self) -> Rectangle {
        let toggle = self.toggle_rect();
        Rectangle {
            x: (toggle.x - 52.0).max(4.0),
            y: toggle.y,
            width: 44.0,
            height: toggle.height,
        }
    }

    fn open_folder_rect(&self) -> Rectangle {
        let theme = self.theme_rect();
        Rectangle {
            x: (theme.x - 124.0).max(4.0),
            y: theme.y,
            width: 116.0,
            height: theme.height,
        }
    }

    fn truncate_text(&self, font: &AppFont, text: &str, max_width: f32, size: f32) -> String {
        if max_width <= 0.0 {
            return String::new();
        }
        if font.measure_width(text, size, 0.0) <= max_width {
            return text.to_string();
        }
        let ellipsis = "...";
        let ellipsis_width = font.measure_width(ellipsis, size, 0.0);
        if ellipsis_width > max_width {
            return String::new();
        }
        let mut out = String::new();
        for ch in text.chars() {
            out.push(ch);
            let projected = font.measure_width(&out, size, 0.0) + ellipsis_width;
            if projected > max_width {
                out.pop();
                break;
            }
        }
        out.push_str(ellipsis);
        out
    }

    fn matches_query(&self, entry: &TreeEntry, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let text = format!("{} {}", entry.display, entry.path.display()).to_lowercase();
        text.contains(query)
    }

    fn visible_entries<'a>(&'a self, entries: &'a [TreeEntry], query: &str) -> Vec<&'a TreeEntry> {
        let mut out = Vec::new();
        let mut skip_depth: Option<usize> = None;
        for entry in entries {
            if let Some(depth) = skip_depth {
                if entry.depth > depth {
                    continue;
                }
                skip_depth = None;
            }
            let is_collapsed = entry.is_dir && self.is_collapsed(&entry.path);
            let matches = self.matches_query(entry, query);
            if matches {
                out.push(entry);
            }
            if is_collapsed {
                skip_depth = Some(entry.depth);
            }
        }
        out
    }

    fn definition_matches<'a>(
        &self,
        defs: &'a HashMap<String, Vec<DefinitionLocation>>,
        query: &str,
    ) -> Vec<(&'a String, &'a Vec<DefinitionLocation>)> {
        if query.is_empty() {
            return Vec::new();
        }
        defs.iter()
            .filter_map(|(name, def)| {
                if name.to_lowercase().contains(query) && !def.is_empty() {
                    Some((name, def))
                } else {
                    None
                }
            })
            .take(15)
            .collect()
    }

    fn content_height(&self, entry_count: usize, match_count: usize) -> f32 {
        let mut height = entry_count as f32 * SIDEBAR_ROW_H;
        if !self.search_query.is_empty() {
            height += 8.0 + 18.0 + match_count as f32 * 20.0;
        }
        height
    }

    fn ensure_entries(&mut self, project: &ProjectModel) {
        if self.entries_dirty || self.entries_version != project.files.len() {
            self.entries.clear();
            self.entries_version = project.files.len();
            let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
            let mut files: Vec<PathBuf> = project.files.iter().cloned().collect();
            files.sort();

            for full in files {
                let rel = full.strip_prefix(&project.root).unwrap_or(&full);
                let comps: Vec<_> = rel.iter().collect();
                if comps.is_empty() {
                    continue;
                }

                let mut cur = PathBuf::new();
                let last_idx = comps.len().saturating_sub(1);
                for (i, comp) in comps.iter().enumerate().take(last_idx) {
                    cur.push(comp);
                    if seen_dirs.insert(cur.clone()) {
                        let depth = i;
                        let name = comp.to_string_lossy().to_string();
                        self.entries.push(TreeEntry {
                            path: cur.clone(),
                            display: name,
                            depth,
                            is_dir: true,
                        });
                    }
                }

                let depth = last_idx;
                let name = comps
                    .last()
                    .map(|c| c.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.entries.push(TreeEntry {
                    path: rel.to_path_buf(),
                    display: name,
                    depth,
                    is_dir: false,
                });
            }
            self.entries_dirty = false;
        }
    }

    pub fn handle_wheel(
        &mut self,
        mouse: Vector2,
        wheel: f32,
        project: &ProjectModel,
        defs: &HashMap<String, Vec<DefinitionLocation>>,
        sidebar_height: f32,
    ) -> bool {
        let width = self.current_width();
        if self.collapsed || mouse.x > width {
            return false;
        }
        let query = self.search_query.to_lowercase();
        self.ensure_entries(project);
        let filtered: Vec<TreeEntry> = self
            .visible_entries(&self.entries, &query)
            .into_iter()
            .cloned()
            .collect();
        let matches = self.definition_matches(defs, &query);
        let start_y = self.list_start_y();
        let content_height = self.content_height(filtered.len(), matches.len());
        let max_scroll = (content_height - (sidebar_height - start_y)).max(0.0);
        self.scroll = (self.scroll - wheel * SIDEBAR_ROW_H).clamp(0.0, max_scroll.max(0.0));
        true
    }

    pub fn handle_click(
        &mut self,
        mouse: Vector2,
        project: &ProjectModel,
        defs: &HashMap<String, Vec<DefinitionLocation>>,
    ) -> Option<SidebarAction> {
        let width = self.current_width();
        if mouse.x > width {
            return None;
        }
        let toggle_rect = self.toggle_rect();
        if point_in_rect(mouse, toggle_rect) {
            return Some(SidebarAction::ToggleCollapse);
        }
        if self.collapsed {
            return Some(SidebarAction::ToggleCollapse);
        }
        let theme_rect = self.theme_rect();
        if point_in_rect(mouse, theme_rect) {
            return Some(SidebarAction::ToggleTheme);
        }
        let open_rect = self.open_folder_rect();
        if point_in_rect(mouse, open_rect) {
            return Some(SidebarAction::OpenFolder);
        }
        let query = self.search_query.to_lowercase();
        self.ensure_entries(project);
        let filtered: Vec<TreeEntry> = self
            .visible_entries(&self.entries, &query)
            .into_iter()
            .cloned()
            .collect();
        let matches = self.definition_matches(defs, &query);
        let mut y = self.list_start_y() - self.scroll;
        for entry in filtered {
            let rect = self.entry_rect(&entry, y);
            if point_in_rect(mouse, rect) {
                if entry.is_dir {
                    self.toggle_dir(&entry.path);
                    return Some(SidebarAction::ToggleDir);
                }
                let full_path = project.root.join(&entry.path);
                return Some(SidebarAction::OpenFile {
                    path: full_path,
                    line: None,
                });
            }
            y += SIDEBAR_ROW_H;
        }

        if !query.is_empty() {
            y += 8.0 + 18.0; // spacing + "Matches" header
            for (_, def) in matches {
                let rect = Rectangle {
                    x: 10.0,
                    y,
                    width: width - 20.0,
                    height: 20.0,
                };
                if point_in_rect(mouse, rect) {
                    if let Some(target) = def.first() {
                        return Some(SidebarAction::OpenFile {
                            path: target.file.clone(),
                            line: Some(target.line),
                        });
                    }
                }
                y += 20.0;
            }
        }
        None
    }

    pub fn draw(
        &mut self,
        d: &mut RaylibDrawHandle,
        font: &AppFont,
        mouse: Vector2,
        project: &ProjectModel,
        defs: &HashMap<String, Vec<DefinitionLocation>>,
        palette: &Palette,
        theme_label: &str,
    ) {
        let width = self.current_width();
        d.draw_rectangle(0, 0, width as i32, d.get_screen_height(), palette.sidebar);
        let toggle_rect = self.toggle_rect();
        let toggle_icon = if self.collapsed {
            Icon::PanelLeftOpen
        } else {
            Icon::PanelLeftClose
        };
        self.icons.render(
            d,
            toggle_icon,
            Vector2::new(toggle_rect.x, toggle_rect.y),
            palette.text,
        );

        if self.collapsed {
            d.draw_line(
                width as i32,
                0,
                width as i32,
                d.get_screen_height(),
                palette.breadcrumb,
            );
            return;
        }

        let theme_rect = self.theme_rect();
        d.draw_rectangle(
            theme_rect.x as i32,
            theme_rect.y as i32,
            theme_rect.width as i32,
            theme_rect.height as i32,
            palette.title,
        );
        if point_in_rect(mouse, theme_rect) {
            d.draw_rectangle_lines(
                theme_rect.x as i32,
                theme_rect.y as i32,
                theme_rect.width as i32,
                theme_rect.height as i32,
                palette.project,
            );
        }
        font.draw_text_ex(
            d,
            theme_label,
            Vector2::new(theme_rect.x + 4.0, theme_rect.y + 1.0),
            FONT_SIZE - 2.0,
            0.0,
            palette.text,
        );
        let open_rect = self.open_folder_rect();
        d.draw_rectangle(
            open_rect.x as i32,
            open_rect.y as i32,
            open_rect.width as i32,
            open_rect.height as i32,
            palette.title,
        );
        if point_in_rect(mouse, open_rect) {
            d.draw_rectangle_lines(
                open_rect.x as i32,
                open_rect.y as i32,
                open_rect.width as i32,
                open_rect.height as i32,
                palette.project,
            );
        }
        let icon_y = open_rect.y + (open_rect.height - self.icons.size() as f32) * 0.5;
        self.icons.render(
            d,
            Icon::FolderOpen,
            Vector2::new(open_rect.x + 4.0, icon_y),
            palette.text,
        );
        font.draw_text_ex(
            d,
            "Open Folder",
            Vector2::new(open_rect.x + self.icons.size() as f32 + 8.0, open_rect.y + 1.0),
            FONT_SIZE - 2.0,
            0.0,
            palette.text,
        );
        font.draw_text_ex(
            d,
            "Project",
            Vector2::new(12.0, 10.0),
            FONT_SIZE + 2.0,
            0.0,
            palette.project,
        );

        let search_rect = self.search_rect();
        d.draw_rectangle(
            search_rect.x as i32,
            search_rect.y as i32,
            search_rect.width as i32,
            search_rect.height as i32,
            palette.search_bg,
        );
        if self.search_focused {
            d.draw_rectangle_lines(
                (search_rect.x - 1.0) as i32,
                (search_rect.y - 1.0) as i32,
                (search_rect.width + 2.0) as i32,
                (search_rect.height + 2.0) as i32,
                palette.project,
            );
        }
        let search_text = if self.search_query.is_empty() {
            "search..."
        } else {
            &self.search_query
        };
        let search_color = if self.search_query.is_empty() {
            palette.line_num
        } else {
            palette.sidebar_text
        };
        font.draw_text_ex(
            d,
            search_text,
            Vector2::new(search_rect.x + 6.0, search_rect.y + 4.0),
            FONT_SIZE,
            0.0,
            search_color,
        );

        let query = self.search_query.to_lowercase();
        let start_y = self.list_start_y();
        self.ensure_entries(project);
        let filtered: Vec<TreeEntry> = self
            .visible_entries(&self.entries, &query)
            .into_iter()
            .cloned()
            .collect();
        let matches = self.definition_matches(defs, &query);
        let visible_height = (d.get_screen_height() as f32 - start_y).max(0.0);
        let content_height = self.content_height(filtered.len(), matches.len());
        let max_scroll = (content_height - visible_height).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        let mut visible_y = start_y - self.scroll;

        let scissor_height = visible_height.max(0.0) as i32;
        {
            let mut scoped = d.begin_scissor_mode(0, start_y as i32, width as i32, scissor_height);
            for entry in filtered {
                let rect = self.entry_rect(&entry, visible_y);
                if rect.y + rect.height < start_y {
                    visible_y += SIDEBAR_ROW_H;
                    continue;
                }
                if rect.y > scoped.get_screen_height() as f32 {
                    break;
                }
                if point_in_rect(mouse, rect) {
                    scoped.draw_rectangle(
                        rect.x as i32,
                        rect.y as i32,
                        rect.width as i32,
                        rect.height as i32,
                        palette.sidebar_highlight,
                    );
                }
                let mut text_x = rect.x + 4.0;
                if entry.is_dir {
                    let arrow = if self.is_collapsed(&entry.path) {
                        Icon::ChevronRight
                    } else {
                        Icon::ChevronDown
                    };
                    self.icons.render(
                        &mut scoped,
                        arrow,
                        Vector2::new(text_x, rect.y + 3.0),
                        palette.sidebar_text,
                    );
                    text_x += self.icons.size() as f32 + 4.0;
                }
                let icon = if entry.is_dir {
                    if self.is_collapsed(&entry.path) {
                        Icon::FolderClosed
                    } else {
                        Icon::FolderOpen
                    }
                } else {
                    Icon::File
                };
                let tint = if entry.is_dir {
                    palette.project
                } else {
                    palette.sidebar_text
                };
                self.icons
                    .render(&mut scoped, icon, Vector2::new(text_x, rect.y + 3.0), tint);
                text_x += self.icons.size() as f32 + 6.0;
                let max_label_width = rect.x + rect.width - text_x - SCROLLBAR_GUTTER;
                let label = self.truncate_text(font, &entry.display, max_label_width, FONT_SIZE);
                font.draw_text_ex(
                    &mut scoped,
                    &label,
                    Vector2::new(text_x, rect.y + 2.0),
                    FONT_SIZE,
                    0.0,
                    palette.sidebar_text,
                );
                visible_y += SIDEBAR_ROW_H;
            }

            let mut y_matches = visible_y + 8.0;
            if !query.is_empty() {
                font.draw_text_ex(
                    &mut scoped,
                    "Matches",
                    Vector2::new(12.0, y_matches),
                    FONT_SIZE,
                    0.0,
                    palette.project,
                );
                y_matches += 18.0;
                for (def_name, def) in matches {
                    let rect = Rectangle {
                        x: 10.0,
                        y: y_matches,
                        width: width - 20.0,
                        height: 20.0,
                    };
                    if point_in_rect(mouse, rect) {
                        scoped.draw_rectangle(
                            rect.x as i32,
                            rect.y as i32,
                            rect.width as i32,
                            rect.height as i32,
                            palette.sidebar_highlight,
                        );
                    }
                    if let Some(target) = def.first() {
                        let label = format!(
                            "{} ({})",
                            def_name,
                            target
                                .module_path
                                .strip_prefix("crate::")
                                .unwrap_or(&target.module_path)
                        );
                        let truncated = self.truncate_text(
                            font,
                            &label,
                            rect.width - SCROLLBAR_GUTTER,
                            FONT_SIZE - 4.0,
                        );
                        font.draw_text_ex(
                            &mut scoped,
                            &truncated,
                            Vector2::new(rect.x + 4.0, y_matches),
                            FONT_SIZE - 4.0,
                            0.0,
                            palette.sidebar_text,
                        );
                    }
                    y_matches += 20.0;
                }
            }
        }

        if content_height > visible_height + f32::EPSILON && visible_height > 0.0 {
            let track_x = width - SCROLLBAR_WIDTH - 4.0;
            let track_y = start_y;
            let track_h = visible_height;
            d.draw_rectangle(
                track_x as i32,
                track_y as i32,
                SCROLLBAR_WIDTH as i32,
                track_h as i32,
                palette.search_bg,
            );
            let scroll_range = (content_height - visible_height).max(1.0);
            let thumb_h =
                (visible_height / content_height * track_h).clamp(SCROLLBAR_MIN_THUMB, track_h);
            let thumb_y = track_y + (self.scroll / scroll_range) * (track_h - thumb_h);
            d.draw_rectangle(
                track_x as i32,
                thumb_y as i32,
                SCROLLBAR_WIDTH as i32,
                thumb_h as i32,
                palette.sidebar_highlight,
            );
        }
        d.draw_line(
            width as i32,
            0,
            width as i32,
            d.get_screen_height(),
            palette.breadcrumb,
        );
    }
}
