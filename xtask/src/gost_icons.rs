//! Original CADCraftGOST icon source: an engineering sheet, flange projection and ESKD title block.
//! SPDX-License-Identifier: MIT OR Apache-2.0
//!
//! Artwork: CADCraftGOST contributors, 2026. No ArtCraft/SolidCraft artwork is used.
//! `cargo xtask gost-icons` reproducibly writes every shipped icon using the workspace's
//! tiny-skia rasterizer and existing ICO packer. No OS-specific image tools are needed.

use std::path::Path;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

const PRODUCT_ID: &str = "io.github.mikiway902.cadcraftgost";
const SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];
const NAVY: [u8; 3] = [18, 43, 55];
const PAPER: [u8; 3] = [240, 247, 241];
const TEAL: [u8; 3] = [34, 181, 165];
const AMBER: [u8; 3] = [246, 180, 67];

struct Canvas {
    pixmap: Pixmap,
    transform: Transform,
}

impl Canvas {
    fn paint(color: [u8; 3]) -> Paint<'static> {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], 255);
        paint
    }

    fn fill(&mut self, path: PathBuilder, color: [u8; 3]) -> Result<(), String> {
        let path = path.finish().ok_or("invalid icon fill path")?;
        self.pixmap.fill_path(&path, &Self::paint(color), FillRule::Winding, self.transform, None);
        Ok(())
    }

    fn line(&mut self, points: &[(f32, f32)], color: [u8; 3], width: f32) -> Result<(), String> {
        let mut path = PathBuilder::new();
        for (index, &(x, y)) in points.iter().enumerate() {
            if index == 0 {
                path.move_to(x, y);
            } else {
                path.line_to(x, y);
            }
        }
        let path = path.finish().ok_or("invalid icon line path")?;
        self.pixmap.stroke_path(&path, &Self::paint(color), &Stroke { width, ..Default::default() }, self.transform, None);
        Ok(())
    }

    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: [u8; 3]) -> Result<(), String> {
        let rect = Rect::from_xywh(x, y, width, height).ok_or("invalid icon rectangle")?;
        let mut path = PathBuilder::new();
        path.push_rect(rect);
        self.fill(path, color)
    }

    fn circle(&mut self, x: f32, y: f32, radius: f32, color: [u8; 3]) -> Result<(), String> {
        let mut path = PathBuilder::new();
        path.push_circle(x, y, radius);
        self.fill(path, color)
    }
}

fn image(size: u32, macos_margin: bool) -> Result<Pixmap, String> {
    if size == 0 || size > 1024 {
        return Err("icon size must be 1..=1024".into());
    }
    let scale = size as f32 / 1024.0 * if macos_margin { 0.82 } else { 1.0 };
    let offset = if macos_margin { size as f32 * 0.09 } else { 0.0 };
    let mut c = Canvas {
        pixmap: Pixmap::new(size, size).ok_or("cannot allocate icon")?,
        transform: Transform::from_row(scale, 0.0, 0.0, scale, offset, offset),
    };
    // A clipped sheet silhouette, not the upstream compass/set-square/grid tile.
    let mut sheet = PathBuilder::new();
    sheet.move_to(96.0, 48.0);
    sheet.line_to(744.0, 48.0);
    sheet.line_to(928.0, 232.0);
    sheet.line_to(928.0, 976.0);
    sheet.line_to(96.0, 976.0);
    sheet.close();
    c.fill(sheet, NAVY)?;
    c.rect(144.0, 96.0, 552.0, 608.0, PAPER)?;
    c.rect(696.0, 280.0, 184.0, 424.0, PAPER)?;
    let mut fold = PathBuilder::new();
    fold.move_to(744.0, 112.0);
    fold.line_to(864.0, 232.0);
    fold.line_to(744.0, 232.0);
    fold.close();
    c.fill(fold, AMBER)?;
    // Orthographic flange projection: two concentric bores, four bolt holes, centre marks.
    c.circle(512.0, 400.0, 208.0, TEAL)?;
    c.circle(512.0, 400.0, 160.0, PAPER)?;
    c.circle(512.0, 400.0, 100.0, NAVY)?;
    c.circle(512.0, 400.0, 52.0, PAPER)?;
    for (x, y) in [(512.0, 260.0), (652.0, 400.0), (512.0, 540.0), (372.0, 400.0)] {
        c.circle(x, y, 22.0, NAVY)?;
    }
    c.line(&[(248.0, 400.0), (284.0, 400.0)], NAVY, 16.0)?;
    c.line(&[(740.0, 400.0), (776.0, 400.0)], NAVY, 16.0)?;
    c.line(&[(512.0, 136.0), (512.0, 172.0)], NAVY, 16.0)?;
    c.line(&[(512.0, 628.0), (512.0, 664.0)], NAVY, 16.0)?;
    // ESKD-inspired lower title block, readable even at small taskbar sizes.
    c.rect(144.0, 752.0, 736.0, 176.0, TEAL)?;
    c.rect(192.0, 800.0, 344.0, 80.0, NAVY)?;
    c.rect(584.0, 800.0, 248.0, 24.0, NAVY)?;
    c.rect(584.0, 856.0, 112.0, 24.0, NAVY)?;
    c.rect(744.0, 856.0, 88.0, 24.0, NAVY)?;
    Ok(c.pixmap)
}

fn png(size: u32, macos_margin: bool) -> Result<Vec<u8>, String> {
    image(size, macos_margin)?.encode_png().map_err(|e| format!("encode icon: {e}"))
}

fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Modern PNG-compressed ICNS elements (Apple's public icon-family container format).
fn icns() -> Result<Vec<u8>, String> {
    let mut output = Vec::from(*b"icns\0\0\0\0");
    for (kind, size) in [(b"icp4", 16), (b"icp5", 32), (b"icp6", 64), (b"ic07", 128), (b"ic08", 256), (b"ic09", 512), (b"ic10", 1024)] {
        let bytes = png(size, true)?;
        let length = u32::try_from(bytes.len() + 8).map_err(|_| "ICNS image too large")?;
        output.extend_from_slice(kind);
        output.extend_from_slice(&length.to_be_bytes());
        output.extend_from_slice(&bytes);
    }
    let length = u32::try_from(output.len()).map_err(|_| "ICNS container too large")?;
    output.get_mut(4..8).ok_or("invalid ICNS header")?.copy_from_slice(&length.to_be_bytes());
    Ok(output)
}

pub fn run(root: &Path) -> Result<(), String> {
    let dir = root.join("assets/app-icon");
    for &size in SIZES {
        let folder = dir.join(format!("hicolor/{size}x{size}/apps"));
        save(&folder.join(format!("{PRODUCT_ID}.png")), &png(size, false)?)?;
        // No original-product icon remains in the icon tree copied into packages.
        let original = folder.join("ai.storyteller.cadcraft.png");
        match std::fs::remove_file(&original) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("remove {}: {e}", original.display())),
        }
    }
    for size in [64, 256, 1024] {
        save(&dir.join(format!("cadcraft-{size}.png")), &png(size, false)?)?;
    }
    save(&dir.join("cadcraft-macos-512.png"), &png(512, true)?)?;
    let images = [16, 20, 24, 32, 40, 48, 64, 128, 256].into_iter().map(|size| png(size, false)).collect::<Result<Vec<_>, _>>()?;
    save(&dir.join("cadcraft.ico"), &crate::ico::pack(&images)?)?;
    save(&dir.join("cadcraft.icns"), &icns()?)?;
    println!("Original CADCraftGOST icons written to {}", dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_artwork_has_own_palette_and_macos_margin() {
        let desktop = image(256, false).expect("desktop icon");
        let macos = image(256, true).expect("macOS icon");
        assert_eq!(desktop.pixel(128, 208).expect("title block").alpha(), 255);
        assert_eq!(macos.pixel(24, 24).expect("margin").alpha(), 0);
        assert_ne!(desktop.data(), macos.data());
        assert_eq!(png(32, false).expect("PNG"), png(32, false).expect("same PNG"));
        assert!(image(0, false).is_err());
        assert!(image(1025, false).is_err());
    }

    #[test]
    fn platform_containers_preserve_png_sizes() {
        let icons = [16, 32, 256].into_iter().map(|size| png(size, false).expect("PNG")).collect::<Vec<_>>();
        let ico = crate::ico::pack(&icons).expect("ICO");
        assert_eq!(ico.get(..6), Some([0, 0, 1, 0, 3, 0].as_slice()));
        let icns = icns().expect("ICNS");
        assert_eq!(icns.get(..4), Some(b"icns".as_slice()));
        let mut cursor = 8;
        let mut sizes = Vec::new();
        while let Some(header) = icns.get(cursor..cursor + 8) {
            let length = u32::from_be_bytes(header[4..8].try_into().expect("element length")) as usize;
            let image = icns.get(cursor + 8..cursor + length).expect("element PNG");
            sizes.push(crate::ico::png_size(image).expect("PNG size").0);
            cursor += length;
        }
        assert_eq!(sizes, [16, 32, 64, 128, 256, 512, 1024]);
        assert_eq!(cursor, icns.len());
    }
}
