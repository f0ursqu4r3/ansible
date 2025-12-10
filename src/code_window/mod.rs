mod metrics;
mod render;
mod types;

pub use metrics::{clamp_window_scroll, content_metrics, metrics_for};
pub use render::{hit_test_calls, is_over_call};
pub use types::{
    CallOrigin, CallRef, CodeViewKind, CodeWindow, MIN_WINDOW_H, MIN_WINDOW_W, SCROLLBAR_MIN_THUMB,
    SCROLLBAR_PADDING, SCROLLBAR_THICKNESS,
};
