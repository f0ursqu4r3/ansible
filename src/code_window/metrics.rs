use crate::constants::{BREADCRUMB_HEIGHT, CODE_X_OFFSET, LINE_HEIGHT, TITLE_BAR_HEIGHT};
use crate::model::{ParsedFile, ProjectModel};

use super::types::{
    CodeWindow, ContentMetrics, CONTENT_PADDING, RIGHT_TEXT_PAD, SCROLLBAR_PADDING,
    SCROLLBAR_THICKNESS,
};

pub fn metrics_for(project: &ProjectModel, win: &CodeWindow) -> Option<ContentMetrics> {
    let pf = project.parsed.get(&win.file)?;
    Some(content_metrics(pf, win))
}

pub fn content_metrics(pf: &ParsedFile, win: &CodeWindow) -> ContentMetrics {
    let (_, view_lines) = win.view_lines(pf);
    let base_width = (win.size.x - CODE_X_OFFSET - RIGHT_TEXT_PAD).max(32.0);
    let base_height =
        (win.size.y - TITLE_BAR_HEIGHT - BREADCRUMB_HEIGHT - CONTENT_PADDING).max(LINE_HEIGHT);
    let max_width = view_lines.iter().fold(0.0f32, |acc, line| {
        acc.max(crate::estimated_line_width(line))
    });
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
