use raylib::prelude::*;

use crate::constants::{BREADCRUMB_HEIGHT, CODE_X_OFFSET, LINE_HEIGHT, TITLE_BAR_HEIGHT};
use crate::icons::{Icon, Icons};
use crate::model::{
    DefinitionLocation, FunctionCall, ParsedFile, ProjectModel, colorized_segments_with_calls,
};
use crate::theme::Palette;
use crate::{AppFont, FONT_SIZE, draw_segments, token_rect};

use super::metrics::{content_metrics, minimap_geometry};
use super::types::{
    CallOrigin, CodeWindow, SCROLLBAR_MIN_THUMB, SCROLLBAR_PADDING, SCROLLBAR_THICKNESS,
};

impl CodeWindow {
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
        let darker_bg = Color {
            r: (bg.r as f32 * 0.5) as u8,
            g: (bg.g as f32 * 0.5) as u8,
            b: (bg.b as f32 * 0.5) as u8,
            a: bg.a,
        };
        d.draw_rectangle_rounded_lines(win_rect, radius, 10, darker_bg);

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

fn world_to_screen_rect(rect: Rectangle, pan: Vector2, zoom: f32) -> Rectangle {
    Rectangle {
        x: (rect.x + pan.x) * zoom,
        y: (rect.y + pan.y) * zoom,
        width: rect.width * zoom,
        height: rect.height * zoom,
    }
}

fn draw_code(
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

    let start_y = content_rect.y + BREADCRUMB_HEIGHT;
    let text_area_height = metrics.avail_height;
    let top_visible = (win.scroll / LINE_HEIGHT).floor() as usize;
    let lines_visible = ((text_area_height + LINE_HEIGHT) / LINE_HEIGHT).ceil() as usize;
    let bottom = (top_visible + lines_visible + 1).min(view_lines.len());

    let code_scissor_w = (scissor_w - CODE_X_OFFSET).max(1.0);
    let code_scissor_h = (scissor_h - BREADCRUMB_HEIGHT).max(1.0);
    let code_scissor = world_to_screen_rect(
        Rectangle {
            x: content_rect.x + CODE_X_OFFSET,
            y: content_rect.y + BREADCRUMB_HEIGHT,
            width: code_scissor_w,
            height: code_scissor_h,
        },
        pan,
        zoom,
    );
    {
        let mut code_scope = scoped.begin_scissor_mode(
            code_scissor.x as i32,
            code_scissor.y as i32,
            code_scissor.width as i32,
            code_scissor.height as i32,
        );
        let mut y = start_y - (win.scroll % LINE_HEIGHT);
        for idx in top_visible..bottom {
            let line_idx = view_start + idx;
            let text_start_x = content_rect.x + CODE_X_OFFSET - win.scroll_x;

            let calls: Vec<&FunctionCall> = file.calls_on_line(line_idx).collect();
            let segments = colorized_segments_with_calls(file, line_idx, &calls, palette);
            draw_segments(&mut code_scope, font, text_start_x, y, &segments);
            y += LINE_HEIGHT;
        }
    }

    let gutter_width = CODE_X_OFFSET - 4.0;
    scoped.draw_rectangle(
        content_rect.x as i32,
        start_y as i32,
        gutter_width as i32,
        (metrics.avail_height + LINE_HEIGHT) as i32,
        palette.window,
    );
    let mut y_nums = start_y - (win.scroll % LINE_HEIGHT);
    for idx in top_visible..bottom {
        let line_idx = view_start + idx;
        font.draw_text_ex(
            &mut scoped,
            &format!("{:>4}", line_idx + 1),
            Vector2::new(content_rect.x + 4.0, y_nums),
            FONT_SIZE - 2.0,
            0.0,
            palette.line_num,
        );
        y_nums += LINE_HEIGHT;
    }

    let breadcrumb_rect = Rectangle {
        x: content_rect.x,
        y: content_rect.y,
        width: scissor_w,
        height: BREADCRUMB_HEIGHT,
    };
    scoped.draw_rectangle_rec(breadcrumb_rect, palette.window);

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
    drop(scoped);

    if let Some(mini) = win.minimap_rect_at(&metrics, Vector2::new(0.0, 0.0)) {
        let geo = minimap_geometry(win, &metrics, mini);
        d.draw_rectangle(
            mini.x as i32,
            mini.y as i32,
            mini.width as i32,
            mini.height as i32,
            palette.window,
        );
        let mini_scissor = world_to_screen_rect(mini, pan, zoom);
        let mut scoped = d.begin_scissor_mode(
            mini_scissor.x as i32,
            mini_scissor.y as i32,
            mini_scissor.width as i32,
            mini_scissor.height as i32,
        );
        let start_idx = ((mini.y - geo.content_top) / geo.line_step)
            .floor()
            .max(0.0) as usize;
        for (idx, _line) in view_lines.iter().enumerate().skip(start_idx) {
            let line_y = geo.content_top + idx as f32 * geo.line_step;
            if line_y > mini.y + mini.height {
                break;
            }
            let line_idx = view_start + idx;
            let line = &file.lines[line_idx];
            if line.trim().is_empty() {
                continue;
            }
            let calls: Vec<&FunctionCall> = file.calls_on_line(line_idx).collect();
            let segments = colorized_segments_with_calls(file, line_idx, &calls, palette);
            let mut x = mini.x + 2.0;
            let char_w = (2.0 * geo.scale).max(1.0);
            for (text, color) in segments {
                for ch in text.chars() {
                    if ch.is_whitespace() {
                        x += char_w;
                        continue;
                    }
                    let h = if ch.is_uppercase() { 2.0 } else { 1.0 };
                    let y = line_y + ((geo.line_step - h).max(0.0) * 0.5);
                    scoped.draw_rectangle_rec(
                        Rectangle {
                            x,
                            y,
                            width: char_w,
                            height: h,
                        },
                        color,
                    );
                    x += char_w;
                    if x > mini.x + mini.width - 2.0 {
                        break;
                    }
                }
                if x > mini.x + mini.width - 2.0 {
                    break;
                }
            }
        }
        drop(scoped);

        d.draw_rectangle(
            mini.x as i32,
            geo.view_y as i32,
            mini.width as i32,
            geo.view_h as i32,
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
        if crate::point_in_rect(mouse, rect) {
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

pub fn is_over_call(
    font: &AppFont,
    file: &ParsedFile,
    win: &CodeWindow,
    mouse: Vector2,
    project: &ProjectModel,
) -> bool {
    hit_test_calls(font, file, win, mouse, project).is_some()
}
