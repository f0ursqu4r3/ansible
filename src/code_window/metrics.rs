use crate::constants::{BREADCRUMB_HEIGHT, CODE_X_OFFSET, LINE_HEIGHT, TITLE_BAR_HEIGHT};
use crate::model::{ParsedFile, ProjectModel};
use raylib::prelude::Rectangle;

use super::types::{
    CONTENT_PADDING, CodeWindow, ContentMetrics, RIGHT_TEXT_PAD, SCROLLBAR_PADDING,
    SCROLLBAR_THICKNESS,
};

pub fn metrics_for(project: &ProjectModel, win: &CodeWindow) -> Option<ContentMetrics> {
    let pf = project.parsed.get(&win.file)?;
    if let Some((size, kind, fold_version, cached)) = win.metrics_cache.borrow().as_ref() {
        if *size == win.size && *kind == win.view_kind && *fold_version == win.fold_version {
            return Some(cached.clone());
        }
    }
    let metrics = content_metrics(pf, win);
    *win.metrics_cache.borrow_mut() =
        Some((win.size, win.view_kind.clone(), win.fold_version, metrics.clone()));
    Some(metrics)
}

pub fn content_metrics(pf: &ParsedFile, win: &CodeWindow) -> ContentMetrics {
    let visible_indices = win.visible_line_indices(pf);
    let mut view_lines = Vec::with_capacity(visible_indices.len());
    let mut max_width = 0.0f32;
    for idx in visible_indices {
        if let Some(line) = pf.lines.get(idx) {
            let mut width = crate::estimated_line_width(line);
            if win.collapsed_fold_with_body(idx).is_some() {
                width += crate::estimated_line_width(" ...");
            }
            max_width = max_width.max(width);
            view_lines.push(line);
        }
    }
    let base_width = (win.size.x - CODE_X_OFFSET - RIGHT_TEXT_PAD).max(32.0);
    let base_height =
        (win.size.y - TITLE_BAR_HEIGHT - BREADCRUMB_HEIGHT - CONTENT_PADDING).max(LINE_HEIGHT);
    let total_height = view_lines.len() as f32 * LINE_HEIGHT;

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

#[derive(Clone, Debug)]
pub struct MinimapGeometry {
    pub line_step: f32,
    pub scale: f32,
    pub view_h: f32,
    pub view_y: f32,
    pub max_view_y: f32,
    pub content_top: f32,
}

pub fn minimap_geometry(
    win: &CodeWindow,
    metrics: &ContentMetrics,
    mini: Rectangle,
) -> MinimapGeometry {
    let raw_scale = (mini.height / metrics.total_height.max(1.0)).max(0.001);
    let line_step = (LINE_HEIGHT * raw_scale).clamp(1.0, 2.0);
    let scale = line_step / LINE_HEIGHT;
    let content_height = metrics.total_height * scale;
    let mut view_h = (metrics.avail_height * scale).clamp(4.0, mini.height);
    if content_height > 0.0 {
        view_h = view_h.min(content_height);
    }

    let scroll_range = metrics.max_scroll_y();
    let scroll_frac = if scroll_range > 0.0 {
        (win.scroll / scroll_range).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let content_track = (content_height - view_h).max(0.0);
    let mini_track = (mini.height - view_h).max(0.0);
    let track_h = content_track.min(mini_track);

    let view_y = mini.y + scroll_frac * track_h;

    let offset_max = (content_height - mini.height).max(0.0);
    let content_top = mini.y - scroll_frac * offset_max;
    let max_view_y = mini.y + track_h;

    MinimapGeometry {
        line_step,
        scale,
        view_h,
        view_y,
        max_view_y,
        content_top,
    }
}
