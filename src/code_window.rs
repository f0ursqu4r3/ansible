use std::path::PathBuf;

use raylib::prelude::*;

use crate::constants::{BREADCRUMB_HEIGHT, CODE_X_OFFSET, LINE_HEIGHT, TITLE_BAR_HEIGHT};
use crate::model::{ParsedFile, ProjectModel};

pub const RESIZE_HANDLE: f32 = 14.0;
pub const MIN_WINDOW_W: f32 = 320.0;
pub const MIN_WINDOW_H: f32 = 220.0;
pub const SCROLLBAR_THICKNESS: f32 = 8.0;
pub const SCROLLBAR_PADDING: f32 = 4.0;
pub const SCROLLBAR_MIN_THUMB: f32 = 18.0;
pub const CONTENT_PADDING: f32 = 8.0;
pub const RIGHT_TEXT_PAD: f32 = 24.0;

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
    pub is_resizing: bool,
    pub resize_offset: Vector2,
}

#[derive(Clone, Debug)]
pub enum CodeViewKind {
    FullFile,
    SingleFn { start: usize, end: usize },
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

pub fn metrics_for(project: &ProjectModel, win: &CodeWindow) -> Option<ContentMetrics> {
    let pf = project.parsed.get(&win.file)?;
    Some(content_metrics(pf, win))
}

pub fn content_metrics(pf: &ParsedFile, win: &CodeWindow) -> ContentMetrics {
    let base_width = (win.size.x - CODE_X_OFFSET - RIGHT_TEXT_PAD).max(32.0);
    let base_height = (win.size.y - TITLE_BAR_HEIGHT - BREADCRUMB_HEIGHT - CONTENT_PADDING)
        .max(LINE_HEIGHT);
    let max_width = pf
        .lines
        .iter()
        .fold(0.0f32, |acc, line| acc.max(crate::estimated_line_width(line)));
    let total_height = pf.lines.len() as f32 * LINE_HEIGHT;

    let mut avail_width = base_width;
    let mut avail_height = base_height;
    let mut show_v = total_height > avail_height + 0.5;
    let mut show_h = max_width > avail_width + 0.5;
    for _ in 0..3 {
        let next_width = (base_width
            - if show_v {
                SCROLLBAR_THICKNESS + SCROLLBAR_PADDING
            } else {
                0.0
            })
        .max(32.0);
        let next_height = (base_height
            - if show_h {
                SCROLLBAR_THICKNESS + SCROLLBAR_PADDING
            } else {
                0.0
            })
        .max(LINE_HEIGHT);
        if (next_width - avail_width).abs() < 0.25 && (next_height - avail_height).abs() < 0.25 {
            avail_width = next_width;
            avail_height = next_height;
            break;
        }
        avail_width = next_width;
        avail_height = next_height;
        show_v = total_height > avail_height + 0.5;
        show_h = max_width > avail_width + 0.5;
    }
    show_v = total_height > avail_height + 0.5;
    show_h = max_width > avail_width + 0.5;

    ContentMetrics {
        avail_width,
        avail_height,
        max_width,
        total_height,
        show_v,
        show_h,
    }
}

pub fn clamp_window_scroll(project: &ProjectModel, win: &mut CodeWindow) {
    if let Some(metrics) = metrics_for(project, win) {
        win.scroll = win.scroll.clamp(0.0, metrics.max_scroll_y());
        win.scroll_x = win.scroll_x.clamp(0.0, metrics.max_scroll_x());
    }
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

    pub fn resize_handle_rect(&self) -> Rectangle {
        Rectangle {
            x: self.position.x + self.size.x - RESIZE_HANDLE,
            y: self.position.y + self.size.y - RESIZE_HANDLE,
            width: RESIZE_HANDLE,
            height: RESIZE_HANDLE,
        }
    }
}
