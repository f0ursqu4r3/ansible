use std::cell::RefCell;
use std::path::PathBuf;

use raylib::prelude::*;

use crate::constants::{BREADCRUMB_HEIGHT, CODE_X_OFFSET, LINE_HEIGHT, TITLE_BAR_HEIGHT};
use crate::model::ParsedFile;
use crate::point_in_rect;

pub const RESIZE_HANDLE: f32 = 14.0;
pub const MIN_WINDOW_W: f32 = 320.0;
pub const MIN_WINDOW_H: f32 = 220.0;
pub const SCROLLBAR_THICKNESS: f32 = 8.0;
pub const SCROLLBAR_PADDING: f32 = 4.0;
pub const SCROLLBAR_MIN_THUMB: f32 = 18.0;
pub const CONTENT_PADDING: f32 = 8.0;
pub const RIGHT_TEXT_PAD: f32 = 24.0;
pub const MINIMAP_WIDTH: f32 = 64.0;

#[derive(Clone, Debug)]
pub struct CodeWindow {
    pub file: PathBuf,
    pub title: String,
    pub focus_line: Option<usize>,
    pub view_kind: CodeViewKind,
    pub def_refs: Vec<FunctionRef>,
    pub call_refs: Vec<CallRef>,
    pub folds: Vec<FoldRegion>,
    pub fold_version: u64,
    pub link_from: Option<CallOrigin>,
    pub position: Vector2,
    pub size: Vector2,
    pub scroll: f32,
    pub scroll_x: f32,
    pub is_dragging: bool,
    pub drag_offset: Vector2,
    pub is_resizing: bool,
    pub resize_origin_pos: Vector2,
    pub resize_origin_size: Vector2,
    pub resize_edges: (bool, bool, bool, bool), // left, right, top, bottom
    pub dragging_vscroll: bool,
    pub dragging_hscroll: bool,
    pub dragging_minimap: bool,
    pub drag_start: Vector2,
    pub hover_edges: Option<(bool, bool, bool, bool)>,
    pub metrics_cache: RefCell<Option<(Vector2, CodeViewKind, u64, ContentMetrics)>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CodeViewKind {
    FullFile,
    SingleFn { start: usize, end: usize },
}

#[derive(Clone, Debug)]
pub struct FunctionRef {
    pub name: String,
    pub module_path: String,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub struct CallRef {
    pub name: String,
    pub module_path: String,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub struct FoldRegion {
    pub start: usize,
    pub end: usize,
    pub collapsed: bool,
}

#[derive(Clone, Debug)]
pub struct CallOrigin {
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub struct ContentMetrics {
    pub avail_width: f32,
    pub avail_height: f32,
    pub max_width: f32,
    pub total_height: f32,
    pub show_v: bool,
    pub show_h: bool,
}

impl ContentMetrics {
    pub fn max_scroll_y(&self) -> f32 {
        (self.total_height - self.avail_height).max(0.0)
    }

    pub fn max_scroll_x(&self) -> f32 {
        (self.max_width - self.avail_width).max(0.0)
    }
}

impl CodeWindow {
    pub fn clear_metrics_cache(&self) {
        self.metrics_cache.borrow_mut().take();
    }

    pub fn rect_at(&self, offset: Vector2) -> Rectangle {
        Rectangle {
            x: self.position.x + offset.x,
            y: self.position.y + offset.y,
            width: self.size.x,
            height: self.size.y,
        }
    }

    pub fn content_rect_at(&self, offset: Vector2) -> Rectangle {
        Rectangle {
            x: self.position.x + offset.x,
            y: self.position.y + TITLE_BAR_HEIGHT + offset.y,
            width: self.size.x,
            height: self.size.y - TITLE_BAR_HEIGHT,
        }
    }

    pub fn title_rect_at(&self, offset: Vector2) -> Rectangle {
        Rectangle {
            x: self.position.x + offset.x,
            y: self.position.y + offset.y,
            width: self.size.x,
            height: TITLE_BAR_HEIGHT,
        }
    }

    pub fn minimap_rect_at(&self, metrics: &ContentMetrics, offset: Vector2) -> Option<Rectangle> {
        let content = self.content_rect_at(offset);
        let v_gutter = if metrics.show_v {
            SCROLLBAR_THICKNESS + SCROLLBAR_PADDING
        } else {
            0.0
        };
        let h_gutter = if metrics.show_h {
            SCROLLBAR_THICKNESS + SCROLLBAR_PADDING
        } else {
            0.0
        };
        let x = content.x + content.width - MINIMAP_WIDTH - v_gutter;
        let width = MINIMAP_WIDTH;
        if width <= 0.0 || content.width < width + v_gutter {
            return None;
        }
        let height = content.height - BREADCRUMB_HEIGHT - h_gutter;
        if height <= 0.0 || content.height < BREADCRUMB_HEIGHT {
            return None;
        }
        Some(Rectangle {
            x,
            y: content.y + BREADCRUMB_HEIGHT,
            width,
            height,
        })
    }

    pub fn line_anchor(&self, pf: &ParsedFile, line: usize, prefer_right: bool) -> Option<Vector2> {
        let (start, end) = self.view_range(pf);
        if line > end {
            return None;
        }
        let content = self.content_rect_at(Vector2::new(0.0, 0.0));
        let area_top = content.y + BREADCRUMB_HEIGHT;
        let area_bottom = content.y + content.height;
        let local_idx = line.saturating_sub(start);
        let base_y = area_top + local_idx as f32 * LINE_HEIGHT - self.scroll + LINE_HEIGHT * 0.5;
        let y = base_y.clamp(area_top, area_bottom);
        let x = if prefer_right {
            content.x + content.width
        } else {
            content.x
        };
        Some(Vector2::new(x, y))
    }

    pub fn center_anchor(&self, prefer_right: bool) -> Vector2 {
        let content = self.content_rect_at(Vector2::new(0.0, 0.0));
        let x = if prefer_right {
            content.x + content.width
        } else {
            content.x
        };
        Vector2::new(x, content.y + content.height * 0.5)
    }

    pub fn call_highlight_rect(
        &self,
        pf: &ParsedFile,
        line: usize,
        prefer_right: bool,
    ) -> Option<Rectangle> {
        let local_idx = self.visible_index_for(pf, line)?;
        let content = self.content_rect_at(Vector2::new(0.0, 0.0));
        let area_top = content.y + BREADCRUMB_HEIGHT;
        let y = area_top + local_idx as f32 * LINE_HEIGHT - self.scroll;
        if y + LINE_HEIGHT < area_top || y > content.y + content.height {
            return None;
        }
        let x = if prefer_right {
            content.x + CODE_X_OFFSET - self.scroll_x
        } else {
            content.x + CODE_X_OFFSET - self.scroll_x
        };
        Some(Rectangle {
            x,
            y,
            width: content.width - CODE_X_OFFSET,
            height: LINE_HEIGHT,
        })
    }

    pub fn visible_line_indices(&self, pf: &ParsedFile) -> Vec<usize> {
        if pf.lines.is_empty() {
            return Vec::new();
        }
        let (start, end) = self.view_range(pf);
        let mut indices = Vec::with_capacity(end.saturating_sub(start) + 1);
        for line in start..=end {
            if self.is_line_hidden(line) {
                continue;
            }
            indices.push(line);
        }
        indices
    }

    pub fn visible_index_for(&self, pf: &ParsedFile, line: usize) -> Option<usize> {
        let indices = self.visible_line_indices(pf);
        indices.iter().position(|l| *l == line)
    }

    pub fn view_lines<'a>(&'a self, pf: &'a ParsedFile) -> (Vec<usize>, Vec<&'a String>) {
        let indices = self.visible_line_indices(pf);
        let lines = indices
            .iter()
            .filter_map(|idx| pf.lines.get(*idx))
            .collect();
        (indices, lines)
    }

    pub fn toggle_fold(&mut self, pf: &ParsedFile, line: usize) -> bool {
        // If a fold already exists for this start, flip it.
        if let Some(fold) = self.folds.iter_mut().find(|f| f.start == line) {
            fold.collapsed = !fold.collapsed;
            self.fold_version = self.fold_version.wrapping_add(1);
            self.clear_metrics_cache();
            return true;
        }

        // Only fold real multi-line ranges.
        if let Some(def) = pf
            .defs
            .iter()
            .find(|d| d.line == line && d.end_line > d.line)
        {
            self.folds.push(FoldRegion {
                start: def.line,
                end: def.end_line,
                collapsed: true,
            });
            self.fold_version = self.fold_version.wrapping_add(1);
            self.clear_metrics_cache();
            return true;
        }
        false
    }

    pub fn is_fold_collapsed(&self, line: usize) -> bool {
        self.folds
            .iter()
            .any(|f| f.start == line && f.collapsed)
    }

    pub fn is_foldable_line(&self, pf: &ParsedFile, line: usize) -> bool {
        self.folds.iter().any(|f| f.start == line)
            || pf.defs
                .iter()
                .any(|d| d.line == line && d.end_line > d.line)
    }

    fn is_line_hidden(&self, line: usize) -> bool {
        self.folds.iter().any(|f| f.collapsed && line > f.start && line <= f.end)
    }

    pub fn view_range(&self, pf: &ParsedFile) -> (usize, usize) {
        if pf.lines.is_empty() {
            return (0, 0);
        }
        let last = pf.lines.len().saturating_sub(1);
        match self.view_kind {
            CodeViewKind::FullFile => (0, last),
            CodeViewKind::SingleFn { start, end } => {
                let s = start.min(last);
                let e = end.min(last);
                if s > e { (e, e) } else { (s, e) }
            }
        }
    }

    pub fn update_refs(&mut self, pf: &ParsedFile) {
        let (start, end) = self.view_range(pf);
        self.def_refs = pf
            .defs
            .iter()
            .filter(|d| d.line >= start && d.line <= end)
            .map(|d| FunctionRef {
                name: d.name.clone(),
                module_path: d.module_path.clone(),
                line: d.line,
            })
            .collect();
        self.call_refs = pf
            .calls
            .iter()
            .filter(|c| c.line >= start && c.line <= end)
            .map(|c| CallRef {
                name: c.name.clone(),
                module_path: c.module_path.clone(),
                line: c.line,
            })
            .collect();
    }

    pub fn hit_test(&self, mouse: Vector2) -> bool {
        let margin = RESIZE_HANDLE * 0.6;
        let rect = Rectangle {
            x: self.position.x - margin / 2.0,
            y: self.position.y - margin / 2.0,
            width: self.size.x + margin,
            height: self.size.y + margin,
        };
        point_in_rect(mouse, rect)
    }

    pub fn hit_resize_edges(
        &self,
        mouse: Vector2,
        offset: Vector2,
    ) -> Option<(bool, bool, bool, bool)> {
        let margin = RESIZE_HANDLE * 0.6;
        let rect = Rectangle {
            x: self.position.x + offset.x - margin / 2.0,
            y: self.position.y + offset.y - margin / 2.0,
            width: self.size.x + margin,
            height: self.size.y + margin,
        };
        if unsafe { !raylib::ffi::CheckCollisionPointRec(mouse.into(), rect.into()) } {
            return None;
        }
        let inset_rect = Rectangle {
            x: rect.x + margin / 2.0,
            y: rect.y + margin / 2.0,
            width: (rect.width - margin).max(0.0),
            height: (rect.height - margin).max(0.0),
        };
        let near_left = mouse.x >= rect.x && mouse.x < rect.x + margin;
        let near_right = mouse.x <= rect.x + rect.width && mouse.x > rect.x + rect.width - margin;
        let near_top = mouse.y >= rect.y && mouse.y < rect.y + margin;
        let near_bottom =
            mouse.y <= rect.y + rect.height && mouse.y > rect.y + rect.height - margin;

        // Avoid corners spilling into opposite edges by preferring corner combos.
        let corner = (near_left || near_right) && (near_top || near_bottom);
        if !corner
            && unsafe { raylib::ffi::CheckCollisionPointRec(mouse.into(), inset_rect.into()) }
        {
            return None;
        }
        if near_left || near_right || near_top || near_bottom {
            return Some((near_left, near_right, near_top, near_bottom));
        }
        None
    }
}
