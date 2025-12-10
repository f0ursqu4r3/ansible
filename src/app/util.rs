use raylib::prelude::*;

pub fn cursor_for_edges(edges: (bool, bool, bool, bool)) -> MouseCursor {
    let (l, r, t, b) = edges;
    match (l, r, t, b) {
        (true, false, true, false) | (false, true, false, true) => {
            MouseCursor::MOUSE_CURSOR_RESIZE_NWSE
        }
        (true, false, false, true) | (false, true, true, false) => {
            MouseCursor::MOUSE_CURSOR_RESIZE_NESW
        }
        (true, false, false, false) | (false, true, false, false) => {
            MouseCursor::MOUSE_CURSOR_RESIZE_EW
        }
        (false, false, true, false) | (false, false, false, true) => {
            MouseCursor::MOUSE_CURSOR_RESIZE_NS
        }
        _ => MouseCursor::MOUSE_CURSOR_DEFAULT,
    }
}

pub fn min_distance_to_cubic(points: &[Vector2; 4], p: Vector2) -> f32 {
    let mut min_d2 = f32::MAX;
    let mut prev = points[0];
    let steps = 24;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let cur = cubic_point(points, t);
        let d2 = dist2_point_segment(p, prev, cur);
        if d2 < min_d2 {
            min_d2 = d2;
        }
        prev = cur;
    }
    min_d2.sqrt()
}

fn cubic_point(p: &[Vector2; 4], t: f32) -> Vector2 {
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let t2 = t * t;
    Vector2::new(
        mt2 * mt * p[0].x + 3.0 * mt2 * t * p[1].x + 3.0 * mt * t2 * p[2].x + t2 * t * p[3].x,
        mt2 * mt * p[0].y + 3.0 * mt2 * t * p[1].y + 3.0 * mt * t2 * p[2].y + t2 * t * p[3].y,
    )
}

fn dist2_point_segment(p: Vector2, a: Vector2, b: Vector2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let ab_len2 = ab.x * ab.x + ab.y * ab.y;
    if ab_len2 <= f32::EPSILON {
        return ap.x * ap.x + ap.y * ap.y;
    }
    let t = ((ap.x * ab.x + ap.y * ab.y) / ab_len2).clamp(0.0, 1.0);
    let proj = Vector2::new(a.x + ab.x * t, a.y + ab.y * t);
    let d = p - proj;
    d.x * d.x + d.y * d.y
}
