use std::collections::HashMap;

use anyhow::{Result, anyhow};
use raylib::prelude::*;
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg,
};

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
pub enum Icon {
    ArrowUp,
    ChevronDown,
    ChevronRight,
    Ellipsis,
    EllipsisVertical,
    File,
    FolderClosed,
    FolderOpen,
    Folder,
    HandGrab,
    Hand,
    Menu,
    MousePointer,
    MoveDiagonalNesw,
    MoveDiagonalNwse,
    MoveHorizontal,
    MoveVertical,
    Move,
    Pointer,
    Close,
    ZoomIn,
    ZoomOut,
}
pub struct Icons {
    svgs: HashMap<Icon, Option<Texture2D>>,
    size: u32,
}

impl Icons {
    pub fn load(rl: &mut RaylibHandle, thread: &RaylibThread, size: u32) -> Self {
        let mut svgs = HashMap::new();
        svgs.insert(
            Icon::ArrowUp,
            load_svg_texture(rl, thread, "data/icons/arrow-up.svg", size),
        );
        svgs.insert(
            Icon::ChevronDown,
            load_svg_texture(rl, thread, "data/icons/chevron-down.svg", size),
        );
        svgs.insert(
            Icon::ChevronRight,
            load_svg_texture(rl, thread, "data/icons/chevron-right.svg", size),
        );
        svgs.insert(
            Icon::Ellipsis,
            load_svg_texture(rl, thread, "data/icons/ellipsis.svg", size),
        );
        svgs.insert(
            Icon::EllipsisVertical,
            load_svg_texture(rl, thread, "data/icons/ellipsis-vertical.svg", size),
        );
        svgs.insert(
            Icon::File,
            load_svg_texture(rl, thread, "data/icons/file.svg", size),
        );
        svgs.insert(
            Icon::Folder,
            load_svg_texture(rl, thread, "data/icons/folder.svg", size),
        );
        svgs.insert(
            Icon::FolderOpen,
            load_svg_texture(rl, thread, "data/icons/folder-open.svg", size),
        );
        svgs.insert(
            Icon::FolderClosed,
            load_svg_texture(rl, thread, "data/icons/folder-closed.svg", size),
        );
        svgs.insert(
            Icon::HandGrab,
            load_svg_texture(rl, thread, "data/icons/hand-grab.svg", size),
        );
        svgs.insert(
            Icon::Hand,
            load_svg_texture(rl, thread, "data/icons/hand.svg", size),
        );
        svgs.insert(
            Icon::Menu,
            load_svg_texture(rl, thread, "data/icons/menu.svg", size),
        );
        svgs.insert(
            Icon::MousePointer,
            load_svg_texture(rl, thread, "data/icons/mouse-pointer.svg", size),
        );
        svgs.insert(
            Icon::MoveDiagonalNesw,
            load_svg_texture(rl, thread, "data/icons/move-diagonal-nesw.svg", size),
        );
        svgs.insert(
            Icon::MoveDiagonalNwse,
            load_svg_texture(rl, thread, "data/icons/move-diagonal-nwse.svg", size),
        );
        svgs.insert(
            Icon::MoveHorizontal,
            load_svg_texture(rl, thread, "data/icons/move-horizontal.svg", size),
        );
        svgs.insert(
            Icon::MoveVertical,
            load_svg_texture(rl, thread, "data/icons/move-vertical.svg", size),
        );
        svgs.insert(
            Icon::Move,
            load_svg_texture(rl, thread, "data/icons/move.svg", size),
        );
        svgs.insert(
            Icon::Pointer,
            load_svg_texture(rl, thread, "data/icons/pointer.svg", size),
        );
        svgs.insert(
            Icon::Close,
            load_svg_texture(rl, thread, "data/icons/x-mark.svg", size),
        );
        svgs.insert(
            Icon::ZoomIn,
            load_svg_texture(rl, thread, "data/icons/zoom-in.svg", size),
        );
        svgs.insert(
            Icon::ZoomOut,
            load_svg_texture(rl, thread, "data/icons/zoom-out.svg", size),
        );
        Self { svgs, size }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn render(&self, d: &mut RaylibDrawHandle, icon: Icon, pos: Vector2, tint: Color) {
        if let Some(tex) = self.svgs.get(&icon).and_then(|t| t.as_ref()) {
            d.draw_texture_ex(
                tex,
                pos,
                0.0,
                (self.size as f32) / (tex.height as f32),
                tint,
            );
        }
    }
}

fn load_svg_texture(
    rl: &mut RaylibHandle,
    thread: &RaylibThread,
    path: &str,
    size: u32,
) -> Option<Texture2D> {
    let png_bytes = rasterize_svg_to_png(path, size).ok()?;
    let image = Image::load_image_from_mem(".png", &png_bytes).ok()?;
    rl.load_texture_from_image(thread, &image).ok()
}

fn rasterize_svg_to_png(path: &str, size: u32) -> Result<Vec<u8>> {
    let data = std::fs::read(path)?;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&data, &opt)?;
    let mut pixmap = Pixmap::new(size, size).ok_or_else(|| anyhow!("pixmap allocation failed"))?;
    let svg_size = tree.size();
    let scale_x = size as f32 / svg_size.width();
    let scale_y = size as f32 / svg_size.height();
    let transform = Transform::from_scale(scale_x, scale_y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Force a neutral (white) base so tinting works regardless of SVG fill colors.
    let mut rgba = pixmap.data().to_vec();
    for px in rgba.chunks_mut(4) {
        let a = px[3];
        px[0] = 255;
        px[1] = 255;
        px[2] = 255;
        px[3] = a;
    }

    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_data, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgba)?;
    }
    Ok(png_data)
}
