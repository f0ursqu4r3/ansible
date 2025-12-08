use std::collections::HashSet;
use std::path::PathBuf;

use raylib::prelude::*;

use crate::{
    point_in_rect, AppFont, DefinitionLocation, ProjectModel, COLOR_LINE_NUM, COLOR_PROJECT,
    COLOR_SEARCH_BG, COLOR_SIDEBAR, COLOR_SIDEBAR_HIGHLIGHT, COLOR_SIDEBAR_TEXT, FONT_SIZE,
    SIDEBAR_ROW_H, SIDEBAR_WIDTH,
};

#[derive(Clone)]
struct TreeEntry {
    path: PathBuf,
    display: String,
    depth: usize,
    is_dir: bool,
}

pub enum SidebarAction {
    None,
    OpenFile { path: PathBuf, line: Option<usize> },
}

pub struct SidebarState {
    pub search_query: String,
    pub search_focused: bool,
    pub scroll: f32,
}

impl SidebarState {
    pub fn new() -> Self {
        Self {
            search_query: String::new(),
            search_focused: false,
            scroll: 0.0,
        }
    }

    pub fn search_rect(&self) -> Rectangle {
        Rectangle {
            x: 12.0,
            y: 40.0,
            width: SIDEBAR_WIDTH - 24.0,
            height: 26.0,
        }
    }

    fn sidebar_entries(&self, project: &ProjectModel) -> Vec<TreeEntry> {
        let mut entries = Vec::new();
        let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
        let mut files: Vec<PathBuf> = project.files.iter().cloned().collect();
        files.sort();

        for full in files {
            let rel = full.strip_prefix(&project.root).unwrap_or(&full);
            let comps: Vec<_> = rel.iter().collect();
            if comps.is_empty() {
                continue;
            }

            // Emit dirs in chain if not seen.
            let mut cur = PathBuf::new();
            for (i, comp) in comps.iter().enumerate().take(comps.len().saturating_sub(1)) {
                cur.push(comp);
                if seen_dirs.insert(cur.clone()) {
                    let depth = i;
                    let name = comp.to_string_lossy().to_string();
                    entries.push(TreeEntry {
                        path: cur.clone(),
                        display: name,
                        depth,
                        is_dir: true,
                    });
                }
            }

            let depth = comps.len().saturating_sub(1);
            let name = comps
                .last()
                .map(|c| c.to_string_lossy().to_string())
                .unwrap_or_default();
            entries.push(TreeEntry {
                path: rel.to_path_buf(),
                display: name,
                depth,
                is_dir: false,
            });
        }

        entries
    }

    pub fn handle_wheel(
        &mut self,
        mouse: Vector2,
        wheel: f32,
        project: &ProjectModel,
        sidebar_height: f32,
    ) -> bool {
        if mouse.x > SIDEBAR_WIDTH {
            return false;
        }
        let query = self.search_query.to_lowercase();
        let entries = self.sidebar_entries(project);
        let count = if query.is_empty() {
            entries.len() as f32
        } else {
            entries
                .iter()
                .filter(|e| {
                    let text = format!("{} {}", e.display, e.path.display()).to_lowercase();
                    text.contains(&query)
                })
                .count() as f32
        };
        let search_rect = self.search_rect();
        let start_y = search_rect.y + search_rect.height + 10.0;
        let max_scroll = (count * SIDEBAR_ROW_H - (sidebar_height - start_y)).max(0.0);
        self.scroll = (self.scroll - wheel * SIDEBAR_ROW_H).clamp(0.0, max_scroll.max(0.0));
        true
    }

    pub fn handle_click(
        &mut self,
        mouse: Vector2,
        project: &ProjectModel,
        defs: &std::collections::HashMap<String, Vec<DefinitionLocation>>,
    ) -> Option<SidebarAction> {
        if mouse.x > SIDEBAR_WIDTH {
            return None;
        }
        let query = self.search_query.to_lowercase();
        let search_rect = self.search_rect();
        let mut y = search_rect.y + search_rect.height + 10.0 - self.scroll;
        let entries = self.sidebar_entries(project);
        let filtered: Vec<&TreeEntry> = if query.is_empty() {
            entries.iter().collect()
        } else {
            entries
                .iter()
                .filter(|e| {
                    let text = format!("{} {}", e.display, e.path.display()).to_lowercase();
                    text.contains(&query)
                })
                .collect()
        };
        for entry in filtered {
            let rect = Rectangle {
                x: 10.0 + (entry.depth as f32 * 14.0),
                y,
                width: SIDEBAR_WIDTH - 20.0 - (entry.depth as f32 * 14.0),
                height: SIDEBAR_ROW_H,
            };
            if point_in_rect(mouse, rect) && !entry.is_dir {
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
            for (_, def) in defs
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
                    return Some(SidebarAction::OpenFile {
                        path: target.file.clone(),
                        line: Some(target.line),
                    });
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
        defs: &std::collections::HashMap<String, Vec<DefinitionLocation>>,
    ) {
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
        let entries = self.sidebar_entries(project);
        let filtered: Vec<&TreeEntry> = if query.is_empty() {
            entries.iter().collect()
        } else {
            entries
                .iter()
                .filter(|e| {
                    let text = format!("{} {}", e.display, e.path.display()).to_lowercase();
                    text.contains(&query)
                })
                .collect()
        };
        let max_scroll =
            (filtered.len() as f32 * SIDEBAR_ROW_H - (d.get_screen_height() as f32 - y)).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll.max(0.0));
        let mut visible_y = y - self.scroll;

        for entry in filtered {
            if visible_y + SIDEBAR_ROW_H < search_rect.y {
                visible_y += SIDEBAR_ROW_H;
                continue;
            }
            if visible_y > d.get_screen_height() as f32 {
                break;
            }
            let rect = Rectangle {
                x: 10.0 + (entry.depth as f32 * 14.0),
                y: visible_y,
                width: SIDEBAR_WIDTH - 20.0 - (entry.depth as f32 * 14.0),
                height: SIDEBAR_ROW_H,
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
            let prefix = if entry.is_dir { "[D] " } else { "[F] " };
            let label = format!("{}{}", prefix, entry.display);
            font.draw_text_ex(
                d,
                &label,
                Vector2::new(rect.x + 4.0, visible_y + 2.0),
                FONT_SIZE - 2.0,
                0.0,
                COLOR_SIDEBAR_TEXT,
            );
            visible_y += SIDEBAR_ROW_H;
        }

        let mut y_matches = visible_y + 8.0;
        if !query.is_empty() {
            font.draw_text_ex(
                d,
                "Matches",
                Vector2::new(12.0, y_matches),
                FONT_SIZE - 2.0,
                0.0,
                COLOR_PROJECT,
            );
            y_matches += 18.0;
            for (def_name, def) in defs
                .iter()
                .filter(|(name, _)| name.to_lowercase().contains(&query))
                .take(15)
            {
                let target = def.first().unwrap();
                let rect = Rectangle {
                    x: 10.0,
                    y: y_matches,
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
                    Vector2::new(rect.x + 4.0, y_matches),
                    FONT_SIZE - 4.0,
                    0.0,
                    COLOR_SIDEBAR_TEXT,
                );
                y_matches += 20.0;
            }
        }
    }
}
