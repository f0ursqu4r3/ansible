use raylib::prelude::*;

use crate::code_window;
use crate::code_window::CodeViewKind;
use crate::constants::SIDEBAR_WIDTH;
use crate::AppFont;

use super::types::{
    MinimapContext, MINIMAP_BTN_GAP, MINIMAP_BTN_H, MINIMAP_BTN_W, MINIMAP_H, MINIMAP_MARGIN,
    MINIMAP_PAD, MINIMAP_W,
};
use super::util::min_distance_to_cubic;
use super::AppState;

impl AppState {
    pub fn draw(&mut self, d: &mut RaylibDrawHandle, font: &AppFont, mouse: Vector2) {
        let mut hover_cursor: Option<MouseCursor> = if self.pan_dragging {
            Some(MouseCursor::MOUSE_CURSOR_RESIZE_ALL)
        } else {
            None
        };
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
                        hover_cursor = Some(super::util::cursor_for_edges(edges));
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

    pub(crate) fn draw_call_links(
        &mut self,
        d: &mut RaylibMode2D<RaylibDrawHandle>,
        world_mouse: Vector2,
    ) {
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
                self.call_links.push(super::types::CallLink {
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

    pub(crate) fn minimap_context(&self, screen_w: f32, _screen_h: f32) -> Option<MinimapContext> {
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

    pub(crate) fn minimap_buttons(&self, ctx: &MinimapContext) -> (Rectangle, Rectangle, Rectangle) {
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

    pub(crate) fn minimap_to_world(&self, mouse: Vector2, ctx: &MinimapContext) -> Vector2 {
        Vector2 {
            x: (mouse.x - ctx.origin.x) / ctx.scale,
            y: (mouse.y - ctx.origin.y) / ctx.scale,
        }
    }

    pub(crate) fn center_view_on(&mut self, world: Vector2, screen_w: f32, screen_h: f32) {
        self.pan = Vector2::new(
            screen_w / (2.0 * self.zoom) - world.x,
            screen_h / (2.0 * self.zoom) - world.y,
        );
    }

    pub(crate) fn zoom_to_rect(&mut self, rect: Rectangle, screen_w: f32, screen_h: f32) {
        let target_zoom = (screen_w / rect.width).min(screen_h / rect.height) * 0.9;
        self.zoom = target_zoom.clamp(super::types::MIN_ZOOM, super::types::MAX_ZOOM);
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
}
