use raylib::prelude::*;
use serde::{Deserialize, Serialize};

use crate::code_window::CallOrigin;
use crate::model::DefinitionLocation;

pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 1.0;
pub const MINIMAP_W: f32 = 220.0;
pub const MINIMAP_H: f32 = 160.0;
pub const MINIMAP_MARGIN: f32 = 10.0;
pub const MINIMAP_PAD: f32 = 8.0;
pub const MINIMAP_BTN_W: f32 = 56.0;
pub const MINIMAP_BTN_H: f32 = 18.0;
pub const MINIMAP_BTN_GAP: f32 = 6.0;

#[derive(Clone, Debug)]
pub struct CallLink {
    pub points: [Vector2; 4],
    pub caller_idx: usize,
    pub line: usize,
    pub hovered: bool,
    pub target_idx: usize,
}

#[derive(Serialize, Deserialize)]
pub struct SavedWindow {
    pub file: String,
    pub view_kind: Option<SavedViewKind>,
    pub position: (f32, f32),
    pub size: (f32, f32),
    pub scroll: f32,
    #[serde(default)]
    pub scroll_x: f32,
    #[serde(default)]
    pub link_from: Option<SavedCallOrigin>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedLayout {
    pub windows: Vec<SavedWindow>,
    #[serde(default)]
    pub sidebar_scroll: f32,
    #[serde(default)]
    pub sidebar_collapsed: Vec<String>,
    #[serde(default)]
    pub sidebar_width: Option<f32>,
    #[serde(default)]
    pub sidebar_hidden: bool,
    #[serde(default)]
    pub theme_mode: Option<ThemeMode>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SavedViewKind {
    FullFile,
    SingleFn {
        start: usize,
        end: usize,
        title: String,
    },
}

#[derive(Serialize, Deserialize)]
pub struct SavedCallOrigin {
    pub file: String,
    pub line: usize,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ThemeMode {
    Application,
    Code,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepMode {
    Off,
    Lazy,
    Eager,
}

impl DepMode {
    pub fn from_env() -> Self {
        match std::env::var("TRACE_VIEWER_DEPS")
            .unwrap_or_else(|_| "lazy".to_string())
            .to_lowercase()
            .as_str()
        {
            "off" | "0" | "false" => DepMode::Off,
            "eager" | "on" | "1" | "true" => DepMode::Eager,
            _ => DepMode::Lazy,
        }
    }

    pub fn initial_include_deps(&self) -> bool {
        matches!(self, DepMode::Eager)
    }
}

impl ThemeMode {
    pub fn toggle(self) -> Self {
        match self {
            ThemeMode::Application => ThemeMode::Code,
            ThemeMode::Code => ThemeMode::Application,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ThemeMode::Application => "App",
            ThemeMode::Code => "Code",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MinimapContext {
    pub rect: Rectangle,
    pub bounds: Rectangle,
    pub scale: f32,
    pub origin: Vector2,
}

#[derive(Clone, Debug)]
pub enum WindowAction {
    None,
    Close,
    OpenDefinition {
        def: DefinitionLocation,
        origin: Option<CallOrigin>,
    },
    ToggleFold {
        line: usize,
    },
    StartDrag(Vector2),
    StartResize {
        edges: (bool, bool, bool, bool),
    },
    StartVScroll {
        grab_offset: f32,
        ratio: f32,
    },
    StartHScroll {
        grab_offset: f32,
        ratio: f32,
    },
    StartMinimap {
        grab_offset: f32,
    },
}
