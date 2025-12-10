use std::path::PathBuf;

use raylib::prelude::*;

use crate::constants::{BREADCRUMB_HEIGHT, CODE_X_OFFSET, LINE_HEIGHT, TITLE_BAR_HEIGHT};
use crate::icons::{Icon, Icons};
use crate::model::{
    DefinitionLocation, FunctionCall, ParsedFile, ProjectModel, colorized_segments_with_calls,
};
use crate::theme::Palette;
use crate::{AppFont, FONT_SIZE, draw_segments, point_in_rect, token_rect};

pub const RESIZE_HANDLE: f32 = 14.0;
pub const MIN_WINDOW_W: f32 = 320.0;
pub const MIN_WINDOW_H: f32 = 220.0;
pub const SCROLLBAR_THICKNESS: f32 = 8.0;
pub const SCROLLBAR_PADDING: f32 = 4.0;
pub const SCROLLBAR_MIN_THUMB: f32 = 18.0;
pub const CONTENT_PADDING: f32 = 8.0;
pub const RIGHT_TEXT_PAD: f32 = 24.0;
pub const MINIMAP_WIDTH: f32 = 64.0;
pub const MINIMAP_PADDING: f32 = 6.0;

#[derive(Clone, Debug)]
pub struct CodeWindow {
    pub id: usize,
    pub file: PathBuf,
    pub title: String,
    pub focus_line: Option<usize>,
    pub view_kind: CodeViewKind,
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
}

#[derive(Clone, Debug)]
pub enum CodeViewKind {
    FullFile,
    SingleFn { start: usize, end: usize },
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

fn world_to_screen_rect(rect: Rectangle, pan: Vector2, zoom: f32) -> Rectangle {
    Rectangle {
        x: (rect.x + pan.x) * zoom,
        y: (rect.y + pan.y) * zoom,
        width: rect.width * zoom,
        height: rect.height * zoom,
    }
}

impl CodeWindow {
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

    pub fn minimap_rect(&self, metrics: &ContentMetrics) -> Option<Rectangle> {
        self.minimap_rect_at(metrics, Vector2::new(0.0, 0.0))
    }

    pub fn minimap_rect_at(&self, metrics: &ContentMetrics, offset: Vector2) -> Option<Rectangle> {
        let content = self.content_rect_at(offset);
        let gutter = if metrics.show_v {
            SCROLLBAR_THICKNESS + SCROLLBAR_PADDING
        } else {
            0.0
        };
        let x = content.x + content.width - MINIMAP_WIDTH - gutter - MINIMAP_PADDING;
        let width = MINIMAP_WIDTH;
        if width <= 0.0 || content.width < width + gutter + MINIMAP_PADDING {
            return None;
        }
        let height = metrics.avail_height;
        if height <= 0.0 {
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
        let (start, end) = self.view_range(pf);
        if line > end {
            return None;
        }
        let content = self.content_rect_at(Vector2::new(0.0, 0.0));
        let area_top = content.y + BREADCRUMB_HEIGHT;
        let local_idx = line.saturating_sub(start);
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

    pub fn view_lines<'a>(&'a self, pf: &'a ParsedFile) -> (usize, &'a [String]) {
        if pf.lines.is_empty() {
            return (0, &[]);
        }
        let (start, end) = self.view_range(pf);
        (start, &pf.lines[start..=end])
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

    pub fn draw_window(
        &self,
        d: &mut RaylibDrawHandle,
        font: &AppFont,
        palette: &Palette,
        icons: &Icons,
        project: &ProjectModel,
        is_top: bool,
        pan: Vector2,
        zoom: f32,
    ) {
        let bg = if is_top {
            palette.window_top
        } else {
            palette.window
        };
        let radius = 0.01;
        let win_rect = self.rect_at(Vector2::new(0.0, 0.0));
        d.draw_rectangle_rounded(win_rect, radius, 10, bg);
        d.draw_rectangle_rounded_lines(win_rect, radius, 10, palette.breadcrumb);

        let title_rect = self.title_rect_at(Vector2::new(0.0, 0.0));
        d.draw_rectangle_rounded(
            Rectangle {
                x: title_rect.x,
                y: title_rect.y,
                width: title_rect.width,
                height: title_rect.height,
            },
            radius,
            12,
            palette.title,
        );
        font.draw_text_ex(
            d,
            &self.title,
            Vector2::new(title_rect.x + 8.0, title_rect.y + 8.0),
            FONT_SIZE,
            0.0,
            palette.text,
        );
        let icon_size = icons.size() as f32;
        let close_rect = Rectangle {
            x: title_rect.x + title_rect.width - icon_size - 8.0,
            y: title_rect.y + (TITLE_BAR_HEIGHT - icon_size) * 0.5,
            width: icon_size,
            height: icon_size,
        };
        icons.render(
            d,
            Icon::Close,
            Vector2::new(close_rect.x, close_rect.y),
            palette.close,
        );

        if let Some(pf) = project.parsed.get(&self.file) {
            draw_code(d, font, pf, self, project, palette, pan, zoom);
        }
    }
}

pub fn draw_code(
    d: &mut RaylibDrawHandle,
    font: &AppFont,
    file: &ParsedFile,
    win: &CodeWindow,
    project: &ProjectModel,
    palette: &Palette,
    pan: Vector2,
    zoom: f32,
) {
    let content_rect = win.content_rect_at(Vector2::new(0.0, 0.0));
    if content_rect.width <= 0.0 || content_rect.height <= 0.0 {
        return;
    }

    let metrics = content_metrics(file, win);
    let (view_start, view_lines) = win.view_lines(file);
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
    let screen_scissor = world_to_screen_rect(
        Rectangle {
            x: content_rect.x,
            y: content_rect.y,
            width: scissor_w,
            height: scissor_h,
        },
        pan,
        zoom,
    );
    let mut scoped = d.begin_scissor_mode(
        screen_scissor.x as i32,
        screen_scissor.y as i32,
        screen_scissor.width as i32,
        screen_scissor.height as i32,
    );

    let mut breadcrumb = project.display_name(&file.path);
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
        palette.breadcrumb,
    );

    let start_y = content_rect.y + BREADCRUMB_HEIGHT;
    let text_area_height = metrics.avail_height;
    let top_visible = (win.scroll / LINE_HEIGHT).floor() as usize;
    let lines_visible = ((text_area_height + LINE_HEIGHT) / LINE_HEIGHT).ceil() as usize;
    let bottom = (top_visible + lines_visible + 1).min(view_lines.len());
    let mut y = start_y - (win.scroll % LINE_HEIGHT);

    let gutter_width = CODE_X_OFFSET - 4.0;
    scoped.draw_rectangle(
        content_rect.x as i32,
        start_y as i32,
        gutter_width as i32,
        (metrics.avail_height + LINE_HEIGHT) as i32,
        palette.window,
    );

    for idx in top_visible..bottom {
        let line_idx = view_start + idx;
        let _line = &file.lines[line_idx];
        let text_start_x = content_rect.x + CODE_X_OFFSET - win.scroll_x;

        let calls: Vec<&FunctionCall> = file.calls_on_line(line_idx).collect();
        let segments = colorized_segments_with_calls(file, line_idx, &calls, palette);
        draw_segments(&mut scoped, font, text_start_x, y, &segments);

        font.draw_text_ex(
            &mut scoped,
            &format!("{:>4}", line_idx + 1),
            Vector2::new(content_rect.x + 4.0, y),
            FONT_SIZE - 2.0,
            0.0,
            palette.line_num,
        );
        y += LINE_HEIGHT;
    }
    drop(scoped);

    if let Some(mini) = win.minimap_rect_at(&metrics, Vector2::new(0.0, 0.0)) {
        d.draw_rectangle(
            mini.x as i32,
            mini.y as i32,
            mini.width as i32,
            mini.height as i32,
            palette.window,
        );
        let scale = mini.height / metrics.total_height.max(1.0);
        let line_scale = (scale * LINE_HEIGHT).max(1.0);
        let block_height = (line_scale * 0.7).clamp(1.0, line_scale);
        let max_width = (mini.width - 4.0).max(1.0);
        let mini_scissor = world_to_screen_rect(mini, pan, zoom);
        let mut scoped = d.begin_scissor_mode(
            mini_scissor.x as i32,
            mini_scissor.y as i32,
            mini_scissor.width as i32,
            mini_scissor.height as i32,
        );
        for (idx, _line) in view_lines.iter().enumerate() {
            let line_y = mini.y + idx as f32 * line_scale;
            if line_y > mini.y + mini.height {
                break;
            }
            let line_idx = view_start + idx;
            let calls: Vec<&FunctionCall> = file.calls_on_line(line_idx).collect();
            let segments = colorized_segments_with_calls(file, line_idx, &calls, palette);
            let mut x = mini.x + 2.0;
            for (text, color) in segments {
                let width = font
                    .measure_width(&text, FONT_SIZE, 0.0)
                    .max(text.len() as f32 * 4.0);
                let w = (width / metrics.max_width.max(1.0)) * max_width;
                if w <= 0.5 {
                    continue;
                }
                scoped.draw_rectangle(
                    x as i32,
                    line_y as i32,
                    w as i32,
                    block_height as i32,
                    color,
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
            palette.sidebar_highlight,
        );
        d.draw_rectangle_lines(
            mini.x as i32,
            mini.y as i32,
            mini.width as i32,
            mini.height as i32,
            palette.title,
        );
    }

    if metrics.show_v {
        let track_x = content_rect.x + content_rect.width - SCROLLBAR_THICKNESS - SCROLLBAR_PADDING;
        let track_y = content_rect.y + BREADCRUMB_HEIGHT;
        let track_h = metrics.avail_height;
        d.draw_rectangle(
            track_x as i32,
            track_y as i32,
            SCROLLBAR_THICKNESS as i32,
            track_h as i32,
            palette.search_bg,
        );
        let scroll_range = metrics.max_scroll_y().max(1.0);
        let denom = metrics.total_height.max(1.0);
        let thumb_h = (metrics.avail_height / denom * track_h).clamp(SCROLLBAR_MIN_THUMB, track_h);
        let thumb_y = track_y + (win.scroll / scroll_range) * (track_h - thumb_h);
        d.draw_rectangle(
            track_x as i32,
            thumb_y as i32,
            SCROLLBAR_THICKNESS as i32,
            thumb_h as i32,
            palette.sidebar_highlight,
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
            palette.search_bg,
        );
        let scroll_range = metrics.max_scroll_x().max(1.0);
        let denom = metrics.max_width.max(1.0);
        let thumb_w = (metrics.avail_width / denom * track_w).clamp(SCROLLBAR_MIN_THUMB, track_w);
        let thumb_x = track_x + (win.scroll_x / scroll_range) * (track_w - thumb_w);
        d.draw_rectangle(
            thumb_x as i32,
            track_y as i32,
            thumb_w as i32,
            SCROLLBAR_THICKNESS as i32,
            palette.sidebar_highlight,
        );
    }
}

pub fn hit_test_calls(
    font: &AppFont,
    file: &ParsedFile,
    win: &CodeWindow,
    mouse: Vector2,
    project: &ProjectModel,
) -> Option<(DefinitionLocation, CallOrigin)> {
    let content_rect = win.content_rect_at(Vector2::new(0.0, 0.0));
    let (view_start, view_lines) = win.view_lines(file);
    let content_top = content_rect.y + BREADCRUMB_HEIGHT;
    let local_y = mouse.y - content_top + win.scroll;
    if local_y < 0.0 {
        return None;
    }
    let line_idx = (local_y / LINE_HEIGHT).floor() as usize;
    if line_idx >= view_lines.len() {
        return None;
    }
    let line_idx = view_start + line_idx;
    let local_idx = line_idx.saturating_sub(view_start);
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
            content_top + (local_idx as f32 * LINE_HEIGHT) - win.scroll,
        );
        if point_in_rect(mouse, rect) {
            if let Some(defs) = project.defs.get(&call.name) {
                if let Some(exact) = defs.iter().find(|d| d.module_path == call.module_path) {
                    return Some((
                        exact.clone(),
                        CallOrigin {
                            file: file.path.clone(),
                            line: line_idx,
                        },
                    ));
                }
                if let Some(first) = defs.first() {
                    return Some((
                        first.clone(),
                        CallOrigin {
                            file: file.path.clone(),
                            line: line_idx,
                        },
                    ));
                }
            }
        }
    }

    None
}

pub fn is_over_call(font: &AppFont, file: &ParsedFile, win: &CodeWindow, mouse: Vector2) -> bool {
    let content_rect = win.content_rect_at(Vector2::new(0.0, 0.0));
    let (view_start, view_lines) = win.view_lines(file);
    let content_top = content_rect.y + BREADCRUMB_HEIGHT;
    let local_y = mouse.y - content_top + win.scroll;
    if local_y < 0.0 {
        return false;
    }
    let line_idx = (local_y / LINE_HEIGHT).floor() as usize;
    if line_idx >= view_lines.len() {
        return false;
    }
    let line_idx = view_start + line_idx;
    let local_idx = line_idx.saturating_sub(view_start);
    let line = &file.lines[line_idx];
    let calls: Vec<&FunctionCall> = file.calls_on_line(line_idx).collect();
    if calls.is_empty() {
        return false;
    }

    for call in calls {
        let rect = token_rect(
            font,
            line,
            call.col,
            call.len,
            content_rect.x + CODE_X_OFFSET - win.scroll_x,
            content_top + (local_idx as f32 * LINE_HEIGHT) - win.scroll,
        );
        if point_in_rect(mouse, rect) {
            return true;
        }
    }

    false
}
