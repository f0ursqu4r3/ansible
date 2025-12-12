use std::path::PathBuf;
use std::time::Instant;

use raylib::prelude::*;

use crate::code_window;
use crate::code_window::{
    CallOrigin, CodeViewKind, SCROLLBAR_MIN_THUMB, SCROLLBAR_PADDING, SCROLLBAR_THICKNESS,
    minimap_geometry,
};
use crate::constants::{CODE_X_OFFSET, LINE_HEIGHT, SIDEBAR_WIDTH};
use crate::{AppFont, point_in_rect};

use super::AppState;
use super::types::{MAX_ZOOM, MIN_ZOOM, WindowAction};
use super::util::min_distance_to_cubic;

impl AppState {
    pub fn handle_input(
        &mut self,
        font: &AppFont,
        mouse: Vector2,
        wheel: f32,
        left_pressed: bool,
        left_down: bool,
        middle_pressed: bool,
        middle_down: bool,
        typed: String,
        backspace: bool,
        shift_down: bool,
        ctrl_down: bool,
        space_down: bool,
        screen_w: f32,
        screen_h: f32,
        meta_close_pressed: bool,
    ) {
        let mut world_mouse = Vector2::new(
            mouse.x / self.zoom - self.pan.x,
            mouse.y / self.zoom - self.pan.y,
        );
        self.last_mouse_world = Some(world_mouse);

        if meta_close_pressed {
            self.windows.pop();
            return;
        }

        let mut double_open: Option<(PathBuf, usize, Option<CallOrigin>)> = None;
        if left_pressed {
            let now = Instant::now();
            let hit_idx = self.windows.iter().rposition(|w| w.hit_test(world_mouse));
            if let Some(idx) = hit_idx {
                let same_win = self.last_click_window == Some(idx);
                let close_pos = self
                    .last_click_pos
                    .map(|p| {
                        let dx = p.x - world_mouse.x;
                        let dy = p.y - world_mouse.y;
                        dx * dx + dy * dy < 16.0
                    })
                    .unwrap_or(false);
                let close_time = self
                    .last_click_time
                    .map(|t| now.duration_since(t) <= std::time::Duration::from_millis(350))
                    .unwrap_or(false);
                if same_win && close_pos && close_time {
                    if let CodeViewKind::SingleFn { start, .. } = self.windows[idx].view_kind {
                        let file = self.windows[idx].file.clone();
                        let origin = self.windows[idx].link_from.clone();
                        double_open = Some((file, start, origin));
                    }
                }
            }
            self.last_click_time = Some(now);
            self.last_click_pos = Some(world_mouse);
            self.last_click_window = hit_idx;
        }

        if let Some((file, line, origin)) = double_open {
            let title = file
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file")
                .to_string();
            self.open_file_with_view(file, Some(line), title, CodeViewKind::FullFile, origin);
            return;
        }

        if ctrl_down && wheel.abs() > f32::EPSILON {
            let factor = 1.0 + wheel * 0.1;
            let new_zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
            if (new_zoom - self.zoom).abs() > f32::EPSILON {
                let world_anchor = Vector2::new(
                    (mouse.x - SIDEBAR_WIDTH) / self.zoom - self.pan.x,
                    mouse.y / self.zoom - self.pan.y,
                );
                self.zoom = new_zoom;
                self.pan = Vector2::new(
                    (mouse.x - SIDEBAR_WIDTH) / self.zoom - world_anchor.x,
                    mouse.y / self.zoom - world_anchor.y,
                );
                world_mouse = Vector2::new(
                    mouse.x / self.zoom - self.pan.x,
                    mouse.y / self.zoom - self.pan.y,
                );
                self.last_mouse_world = Some(world_mouse);
            }
            return;
        }

        if left_pressed {
            let over_window = self
                .windows
                .iter()
                .any(|w| point_in_rect(world_mouse, w.rect_at(Vector2::new(0.0, 0.0))));
            if !over_window {
                if let Some((link_idx, group_len)) = self.hit_call_link(world_mouse) {
                    if let Some(link) = self.call_links.get(link_idx) {
                        let caller_idx = link.caller_idx;
                        if caller_idx < self.windows.len() {
                            let mut scrolled = false;
                            {
                                let win = &mut self.windows[caller_idx];
                                if self.project.parsed.contains_key(&win.file) {
                                    let line = link.line;
                                    let local_line = match win.view_kind {
                                        CodeViewKind::SingleFn { start, .. } => {
                                            line.saturating_sub(start)
                                        }
                                        _ => line,
                                    };
                                    let max_scroll = code_window::metrics_for(&self.project, win)
                                        .map(|m| m.max_scroll_y())
                                        .unwrap_or(0.0);
                                    win.scroll = (local_line as f32 * LINE_HEIGHT - 40.0)
                                        .clamp(0.0, max_scroll);
                                    win.focus_line = Some(line);
                                    scrolled = true;
                                }
                            }
                            if scrolled {
                                self.bring_to_front(caller_idx);
                                self.last_link_cycle =
                                    Some((world_mouse, (link_idx + 1) % group_len));
                                return;
                            }
                        }
                    }
                }
            }
        }

        let pan_initiated = middle_pressed || (space_down && left_pressed);
        if pan_initiated {
            self.pan_dragging = true;
            self.pan_anchor = mouse;
            self.pan_start = self.pan;
        }
        let pan_active = middle_down || (space_down && left_down);
        if !pan_active {
            self.pan_dragging = false;
        }

        if self.pan_dragging {
            let dx = (mouse.x - self.pan_anchor.x) / self.zoom;
            let dy = (mouse.y - self.pan_anchor.y) / self.zoom;
            self.pan = Vector2::new(self.pan_start.x + dx, self.pan_start.y + dy);
            return;
        }

        let minimap_ctx = self.minimap_context(screen_w, screen_h);
        if let Some(ctx) = minimap_ctx {
            let buttons = self.minimap_buttons(&ctx);
            if left_pressed {
                if point_in_rect(mouse, buttons.0) {
                    if let Some(win) = self.windows.last() {
                        self.zoom_to_rect(win.rect_at(Vector2::new(0.0, 0.0)), screen_w, screen_h);
                    }
                    return;
                }
                if point_in_rect(mouse, buttons.1) {
                    if let Some(bounds) = self.world_bounds() {
                        self.zoom_to_rect(bounds, screen_w, screen_h);
                    }
                    return;
                }
                if point_in_rect(mouse, buttons.2) {
                    self.zoom = 1.0;
                    self.pan = Vector2::new(0.0, 0.0);
                    return;
                }
                if point_in_rect(mouse, ctx.rect) {
                    self.minimap_dragging = true;
                    let target = self.minimap_to_world(mouse, &ctx);
                    self.center_view_on(target, screen_w, screen_h);
                    return;
                }
            }
            if self.minimap_dragging && left_down {
                let target = self.minimap_to_world(mouse, &ctx);
                self.center_view_on(target, screen_w, screen_h);
                return;
            }
        }

        if !left_down {
            for w in &mut self.windows {
                w.is_dragging = false;
                w.is_resizing = false;
                w.dragging_vscroll = false;
                w.dragging_hscroll = false;
                w.dragging_minimap = false;
                w.hover_edges = None;
            }
            self.minimap_dragging = false;
        }

        if wheel.abs() > f32::EPSILON {
            if self
                .sidebar
                .handle_wheel(mouse, wheel, &self.project, &self.project.defs, screen_h)
            {
                return;
            }
            for idx in (0..self.windows.len()).rev() {
                let content = self.windows[idx].content_rect_at(Vector2::new(0.0, 0.0));
                if point_in_rect(world_mouse, content) {
                    if shift_down {
                        let max_x = {
                            let win_ref = &self.windows[idx];
                            self.max_scroll_x(win_ref)
                        };
                        let win = &mut self.windows[idx];
                        if let Some(max_x) = max_x {
                            win.scroll_x = (win.scroll_x - wheel * LINE_HEIGHT).clamp(0.0, max_x);
                        }
                    } else {
                        let max_scroll = self.max_scroll(idx);
                        let win = &mut self.windows[idx];
                        win.scroll = (win.scroll - wheel * LINE_HEIGHT).clamp(0.0, max_scroll);
                    }
                    break;
                }
            }
        }

        if left_down {
            for w in &mut self.windows {
                if w.is_dragging {
                    w.position = Vector2::new(
                        world_mouse.x - w.drag_offset.x,
                        world_mouse.y - w.drag_offset.y,
                    );
                }
                if w.is_resizing {
                    let (left, right, top, bottom) = w.resize_edges;
                    let mut new_pos = w.resize_origin_pos;
                    let mut new_size = w.resize_origin_size;
                    let dx = world_mouse.x - w.drag_start.x;
                    let dy = world_mouse.y - w.drag_start.y;
                    if left {
                        let max_x = w.resize_origin_pos.x + w.resize_origin_size.x
                            - code_window::MIN_WINDOW_W;
                        let nx = (w.resize_origin_pos.x + dx).min(max_x);
                        new_size.x = (w.resize_origin_pos.x + w.resize_origin_size.x - nx)
                            .max(code_window::MIN_WINDOW_W);
                        new_pos.x = nx;
                    }
                    if right {
                        new_size.x = (w.resize_origin_size.x + dx).max(code_window::MIN_WINDOW_W);
                    }
                    if top {
                        let max_y = w.resize_origin_pos.y + w.resize_origin_size.y
                            - code_window::MIN_WINDOW_H;
                        let ny = (w.resize_origin_pos.y + dy).min(max_y);
                        new_size.y = (w.resize_origin_pos.y + w.resize_origin_size.y - ny)
                            .max(code_window::MIN_WINDOW_H);
                        new_pos.y = ny;
                    }
                    if bottom {
                        new_size.y = (w.resize_origin_size.y + dy).max(code_window::MIN_WINDOW_H);
                    }
                    w.position = new_pos;
                    w.size = new_size;
                    w.clear_metrics_cache();
                    code_window::clamp_window_scroll(&self.project, w);
                }
                if w.dragging_vscroll {
                    if let Some(metrics) = code_window::metrics_for(&self.project, w) {
                        let track_y = w.position.y
                            + self.pan.y
                            + crate::constants::TITLE_BAR_HEIGHT
                            + crate::constants::BREADCRUMB_HEIGHT;
                        let track_h = metrics.avail_height;
                        if track_h > 0.0 {
                            let thumb_h = (metrics.avail_height / metrics.total_height.max(1.0)
                                * track_h)
                                .clamp(SCROLLBAR_MIN_THUMB, track_h);
                            let denom = (track_h - thumb_h).max(1.0);
                            let scroll_range = metrics.max_scroll_y();
                            let ratio =
                                ((mouse.y - track_y - w.drag_start.y) / denom).clamp(0.0, 1.0);
                            w.scroll = (ratio * scroll_range).clamp(0.0, scroll_range);
                        }
                    }
                }
                if w.dragging_hscroll {
                    if let Some(metrics) = code_window::metrics_for(&self.project, w) {
                        let track_x = w.position.x + self.pan.x + CODE_X_OFFSET;
                        let track_w = metrics.avail_width;
                        if track_w > 0.0 {
                            let thumb_w = (metrics.avail_width / metrics.max_width.max(1.0)
                                * track_w)
                                .clamp(SCROLLBAR_MIN_THUMB, track_w);
                            let denom = (track_w - thumb_w).max(1.0);
                            let scroll_range = metrics.max_scroll_x();
                            let ratio =
                                ((mouse.x - track_x - w.drag_start.x) / denom).clamp(0.0, 1.0);
                            w.scroll_x = (ratio * scroll_range).clamp(0.0, scroll_range);
                        }
                    }
                }
                if w.dragging_minimap {
                    if let Some(metrics) = code_window::metrics_for(&self.project, w) {
                        if let Some(mini) = w.minimap_rect_at(&metrics, Vector2::new(0.0, 0.0)) {
                            let geo = minimap_geometry(w, &metrics, mini);
                            let view_y =
                                (world_mouse.y - w.drag_start.y).clamp(mini.y, geo.max_view_y);
                            let scroll_range = metrics.max_scroll_y().max(0.0);
                            let scroll = ((view_y - mini.y) / geo.scale).clamp(0.0, scroll_range);
                            w.scroll = scroll;
                        }
                    }
                }
                if !w.is_resizing {
                    w.hover_edges = w.hit_resize_edges(world_mouse, Vector2::new(0.0, 0.0));
                }
            }
        }

        if left_pressed {
            let mut handled = false;
            for idx in (0..self.windows.len()).rev() {
                if self.windows[idx].hit_test(world_mouse) {
                    self.bring_to_front(idx);
                    let top_idx = self.windows.len() - 1;
                    let action = self.handle_window_click(font, top_idx, world_mouse);
                    match action {
                        WindowAction::Close => {
                            self.windows.pop();
                        }
                        WindowAction::OpenDefinition { def, origin } => {
                            self.open_definition(def, origin);
                        }
                        WindowAction::StartDrag(offset) => {
                            if let Some(win) = self.windows.last_mut() {
                                win.is_dragging = true;
                                win.drag_offset = offset;
                                win.is_resizing = false;
                            }
                        }
                        WindowAction::StartResize { edges } => {
                            if let Some(win) = self.windows.last_mut() {
                                win.is_resizing = true;
                                win.resize_origin_pos = win.position;
                                win.resize_origin_size = win.size;
                                win.resize_edges = edges;
                                win.drag_start = world_mouse;
                                win.is_dragging = false;
                                win.hover_edges = Some(edges);
                            }
                        }
                        WindowAction::StartVScroll { grab_offset, ratio } => {
                            if let Some(win) = self.windows.last_mut() {
                                win.dragging_vscroll = true;
                                win.drag_start.y = grab_offset;
                                if let Some(metrics) = code_window::metrics_for(&self.project, win)
                                {
                                    let scroll_range = metrics.max_scroll_y();
                                    win.scroll = (ratio * scroll_range).clamp(0.0, scroll_range);
                                }
                                win.dragging_hscroll = false;
                                win.dragging_minimap = false;
                            }
                        }
                        WindowAction::StartHScroll { grab_offset, ratio } => {
                            if let Some(win) = self.windows.last_mut() {
                                win.dragging_hscroll = true;
                                win.drag_start.x = grab_offset;
                                if let Some(metrics) = code_window::metrics_for(&self.project, win)
                                {
                                    let scroll_range = metrics.max_scroll_x();
                                    win.scroll_x = (ratio * scroll_range).clamp(0.0, scroll_range);
                                }
                                win.dragging_vscroll = false;
                                win.dragging_minimap = false;
                            }
                        }
                        WindowAction::StartMinimap { grab_offset } => {
                            if let Some(win) = self.windows.last_mut() {
                                win.dragging_minimap = true;
                                win.drag_start.y = grab_offset;
                                if let Some(metrics) = code_window::metrics_for(&self.project, win)
                                {
                                    if let Some(mini) =
                                        win.minimap_rect_at(&metrics, Vector2::new(0.0, 0.0))
                                    {
                                        let geo = minimap_geometry(win, &metrics, mini);
                                        let view_y = (world_mouse.y - grab_offset)
                                            .clamp(mini.y, geo.max_view_y);
                                        let scroll_range = metrics.max_scroll_y().max(0.0);
                                        let scroll = ((view_y - mini.y) / geo.scale)
                                            .clamp(0.0, scroll_range);
                                        win.scroll = scroll;
                                    }
                                }
                                win.dragging_vscroll = false;
                                win.dragging_hscroll = false;
                            }
                        }
                        WindowAction::None => {}
                    }
                    handled = true;
                    break;
                }
            }

            if !handled {
                if let Some(action) =
                    self.sidebar
                        .handle_click(mouse, &self.project, &self.project.defs)
                {
                    self.handle_sidebar_action(action);
                    handled = true;
                }
            }

            if !handled && point_in_rect(mouse, self.sidebar.search_rect()) {
                self.sidebar.search_focused = true;
            } else if !handled {
                self.sidebar.search_focused = false;
            }
        }

        if self.sidebar.search_focused {
            if backspace {
                self.sidebar.search_query.pop();
            }
            if !typed.is_empty() {
                self.sidebar.search_query.push_str(&typed);
            }
        }

        let resizing_idx = self.windows.iter().position(|w| w.is_resizing);
        if let Some(active_idx) = resizing_idx {
            for (i, w) in self.windows.iter_mut().enumerate() {
                if i == active_idx {
                    w.hover_edges = Some(w.resize_edges);
                } else {
                    w.hover_edges = None;
                }
            }
        } else {
            for w in &mut self.windows {
                w.hover_edges = None;
            }
            for idx in (0..self.windows.len()).rev() {
                let can_hover = {
                    let w = &self.windows[idx];
                    !w.is_dragging
                        && !w.dragging_vscroll
                        && !w.dragging_hscroll
                        && !w.dragging_minimap
                };
                if !can_hover {
                    continue;
                }
                let edges = self.windows[idx].hit_resize_edges(mouse, self.pan);
                if let Some(edges) = edges {
                    if let Some(w) = self.windows.get_mut(idx) {
                        w.hover_edges = Some(edges);
                    }
                    break;
                }
            }
        }
    }

    fn handle_window_click(
        &mut self,
        font: &AppFont,
        idx: usize,
        world_mouse: Vector2,
    ) -> WindowAction {
        let win = &self.windows[idx];
        let title_rect = win.title_rect_at(Vector2::new(0.0, 0.0));
        let icon_size = self.icons.size() as f32;
        let close_rect = Rectangle {
            x: title_rect.x + title_rect.width - icon_size - 8.0,
            y: title_rect.y + (crate::constants::TITLE_BAR_HEIGHT - icon_size) * 0.5,
            width: icon_size,
            height: icon_size,
        };

        if point_in_rect(world_mouse, close_rect) {
            return WindowAction::Close;
        }

        if let Some(edges) = win.hit_resize_edges(world_mouse, Vector2::new(0.0, 0.0)) {
            return WindowAction::StartResize { edges };
        }

        if point_in_rect(world_mouse, title_rect) {
            return WindowAction::StartDrag(Vector2 {
                x: world_mouse.x - win.position.x,
                y: world_mouse.y - win.position.y,
            });
        }

        let content_rect = win.content_rect_at(Vector2::new(0.0, 0.0));
        if !point_in_rect(world_mouse, content_rect) {
            return WindowAction::None;
        }

        if let Some(pf) = self.project.parsed.get(&win.file) {
            let metrics = code_window::content_metrics(pf, win);
            if let Some(mini) = win.minimap_rect_at(&metrics, Vector2::new(0.0, 0.0)) {
                if point_in_rect(world_mouse, mini) {
                    let geo = minimap_geometry(win, &metrics, mini);
                    let view_y = geo.view_y;
                    let view_h = geo.view_h;
                    let grab_offset = if world_mouse.y >= view_y && world_mouse.y <= view_y + view_h
                    {
                        (world_mouse.y - view_y).clamp(0.0, view_h)
                    } else {
                        view_h * 0.5
                    };
                    return WindowAction::StartMinimap { grab_offset };
                }
            }
            if metrics.show_v {
                let track_y = content_rect.y + crate::constants::BREADCRUMB_HEIGHT;
                let track_h = metrics.avail_height;
                if track_h > 0.0 {
                    let track_x = content_rect.x + content_rect.width
                        - SCROLLBAR_THICKNESS
                        - SCROLLBAR_PADDING;
                    let scroll_range = metrics.max_scroll_y();
                    let thumb_h = (metrics.avail_height / metrics.total_height.max(1.0) * track_h)
                        .clamp(SCROLLBAR_MIN_THUMB, track_h);
                    let thumb_y = track_y
                        + if scroll_range > 0.0 {
                            (win.scroll / scroll_range) * (track_h - thumb_h)
                        } else {
                            0.0
                        };
                    let denom = (track_h - thumb_h).max(1.0);
                    let ratio = ((world_mouse.y - track_y - thumb_h * 0.5) / denom).clamp(0.0, 1.0);
                    let grab_offset = (world_mouse.y - thumb_y).clamp(0.0, thumb_h);
                    let v_rect = Rectangle {
                        x: track_x,
                        y: track_y,
                        width: SCROLLBAR_THICKNESS,
                        height: track_h,
                    };
                    if point_in_rect(world_mouse, v_rect) {
                        return WindowAction::StartVScroll { grab_offset, ratio };
                    }
                }
            }
            if metrics.show_h {
                let track_x = content_rect.x + CODE_X_OFFSET;
                let track_w = metrics.avail_width;
                if track_w > 0.0 {
                    let track_y = content_rect.y + content_rect.height
                        - SCROLLBAR_THICKNESS
                        - SCROLLBAR_PADDING;
                    let scroll_range = metrics.max_scroll_x();
                    let thumb_w = (metrics.avail_width / metrics.max_width.max(1.0) * track_w)
                        .clamp(SCROLLBAR_MIN_THUMB, track_w);
                    let thumb_x = track_x
                        + if scroll_range > 0.0 {
                            (win.scroll_x / scroll_range) * (track_w - thumb_w)
                        } else {
                            0.0
                        };
                    let denom = (track_w - thumb_w).max(1.0);
                    let ratio = ((world_mouse.x - track_x - thumb_w * 0.5) / denom).clamp(0.0, 1.0);
                    let grab_offset = (world_mouse.x - thumb_x).clamp(0.0, thumb_w);
                    let h_rect = Rectangle {
                        x: track_x,
                        y: track_y,
                        width: track_w,
                        height: SCROLLBAR_THICKNESS,
                    };
                    if point_in_rect(world_mouse, h_rect) {
                        return WindowAction::StartHScroll { grab_offset, ratio };
                    }
                }
            }
            if metrics.show_v {
                let v_rect = Rectangle {
                    x: content_rect.x + content_rect.width
                        - SCROLLBAR_THICKNESS
                        - SCROLLBAR_PADDING,
                    y: content_rect.y + crate::constants::BREADCRUMB_HEIGHT,
                    width: SCROLLBAR_THICKNESS,
                    height: metrics.avail_height,
                };
                if point_in_rect(world_mouse, v_rect) {
                    return WindowAction::None;
                }
            }
            if metrics.show_h {
                let h_rect = Rectangle {
                    x: content_rect.x + CODE_X_OFFSET,
                    y: content_rect.y + content_rect.height
                        - SCROLLBAR_THICKNESS
                        - SCROLLBAR_PADDING,
                    width: metrics.avail_width,
                    height: SCROLLBAR_THICKNESS,
                };
                if point_in_rect(world_mouse, h_rect) {
                    return WindowAction::None;
                }
            }
            if let Some((def, origin)) =
                code_window::hit_test_calls(font, pf, win, world_mouse, &self.project)
            {
                return WindowAction::OpenDefinition {
                    def,
                    origin: Some(origin),
                };
            }
        }

        WindowAction::None
    }

    fn hit_call_link(&mut self, world_mouse: Vector2) -> Option<(usize, usize)> {
        let tolerance = (10.0 / self.zoom.max(0.001)).clamp(8.0, 32.0);
        let mut hits: Vec<usize> = self
            .call_links
            .iter()
            .enumerate()
            .filter_map(|(i, link)| {
                let dist = min_distance_to_cubic(&link.points, world_mouse);
                if dist <= tolerance { Some(i) } else { None }
            })
            .collect();
        if hits.is_empty() {
            return None;
        }
        hits.sort();
        let mut chosen = hits[0];
        let mut dir: isize = 1;
        if let Some(link) = self.call_links.get(chosen) {
            if let Some(win) = self.windows.get(link.caller_idx) {
                let center_y = win.position.y + win.size.y * 0.5;
                dir = if world_mouse.y < center_y { -1 } else { 1 };
            }
        }
        if let Some((last_pos, last_idx)) = self.last_link_cycle {
            let dx = last_pos.x - world_mouse.x;
            let dy = last_pos.y - world_mouse.y;
            if dx * dx + dy * dy < tolerance * tolerance {
                if let Some(pos) = hits.iter().position(|i| *i == last_idx) {
                    let next_pos = if dir < 0 {
                        (pos + hits.len() - 1) % hits.len()
                    } else {
                        (pos + 1) % hits.len()
                    };
                    chosen = hits[next_pos];
                }
            }
        }
        Some((chosen, hits.len()))
    }
}
