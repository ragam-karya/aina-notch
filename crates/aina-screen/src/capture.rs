// Screen capture for a question. screen1 is always the display under the mouse (or under the user's
// mark). Each shot is resized and JPEG-encoded for the model; the display with the mark also gets the
// mark's box in screenshot pixels, a one-line description and a sharper crop from the full-resolution
// capture. Port of src/capture.js; the JSON matches v1's shots.

use std::io::Cursor;

use aina_input::gesture::{Box2, Mark, describe, region_of};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::imageops::{self, FilterType};
use image::{ExtendedColorType, ImageBuffer, ImageEncoder, Rgba, RgbaImage};
use serde::Serialize;

use crate::grab::grab;
use crate::region::{crop_rect, to_image};
use crate::{Display, Rect};

pub const MAX_EDGE: u32 = 1280;
pub const SINGLE_EDGE: u32 = 1024; // a voice question about one screen: smaller image, faster upload and reply
pub const MARK_EDGE: u32 = 1024; // the zoomed view of a marked region

#[derive(Debug, Clone, Serialize)]
pub struct XY {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkShot {
    pub x1: i64,
    pub y1: i64,
    pub x2: i64,
    pub y2: i64,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: String,
    pub crop_base64: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub index: usize,
    pub display_id: String,
    pub bounds: Rect,
    pub width: u32,
    pub height: u32,
    pub has_cursor: bool,
    pub cursor: Option<XY>,
    pub base64: String,
    pub media_type: &'static str,
    pub mark: Option<MarkShot>,
}

fn center(b: &Box2) -> (f64, f64) {
    ((b.x1 + b.x2) / 2.0, (b.y1 + b.y2) / 2.0)
}

fn round(v: f64) -> i64 {
    (v + 0.5).floor() as i64
}

pub fn jpeg(img: &RgbaImage, quality: u8) -> Result<String, String> {
    let rgb: Vec<u8> = img.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .write_image(&rgb, img.width(), img.height(), ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(STANDARD.encode(out.into_inner()))
}

fn fit(img: &RgbaImage, max_edge: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let scale = (max_edge as f64 / w.max(h) as f64).min(1.0);
    if scale >= 1.0 {
        return img.clone();
    }
    imageops::resize(img, (w as f64 * scale).round() as u32, (h as f64 * scale).round() as u32, FilterType::Triangle)
}

/// Captures for a question. `cursor` is in the overlay space; `mark` is what the user marked while
/// talking. `single`: only the display that matters, at SINGLE_EDGE.
pub fn capture_all(displays: &[Display], cursor: (f64, f64), mark: Option<&Mark>, single: bool) -> Result<Vec<Shot>, String> {
    let region = mark.map(region_of);
    let active = region
        .and_then(|r| displays.iter().find(|d| d.bounds.contains(center(&r).0, center(&r).1)))
        .or_else(|| displays.iter().find(|d| d.bounds.contains(cursor.0, cursor.1)))
        .or_else(|| displays.iter().find(|d| d.primary))
        .or(displays.first())
        .ok_or("no display to capture")?;
    let mut order: Vec<&Display> = displays.iter().collect();
    order.sort_by(|a, b| (a.id != active.id).cmp(&(b.id != active.id)).then(a.bounds.x.total_cmp(&b.bounds.x)));
    if single {
        order = vec![active];
    }
    let max_edge = if single { SINGLE_EDGE } else { MAX_EDGE };

    let mut shots = Vec::new();
    for (i, d) in order.iter().enumerate() {
        let (px, py, pw, ph) = d.phys;
        let Ok(pixels) = grab(px, py, pw as i32, ph as i32) else { continue };
        let Some(full) = ImageBuffer::<Rgba<u8>, _>::from_raw(pw, ph, pixels) else { continue };
        let img = fit(&full, max_edge);
        let (width, height) = img.dimensions();
        let b = d.bounds;
        let has_cursor = b.contains(cursor.0, cursor.1);
        let mark_shot = match (mark, region) {
            (Some(m), Some(r)) if b.contains(center(&r).0, center(&r).1) => Some(mark_on(m, &r, &b, &full, (width, height), i + 1)?),
            _ => None,
        };
        shots.push(Shot {
            index: i + 1,
            display_id: d.id.clone(),
            bounds: b,
            width,
            height,
            has_cursor,
            cursor: has_cursor.then(|| XY { x: round((cursor.0 - b.x) * width as f64 / b.width), y: round((cursor.1 - b.y) * height as f64 / b.height) }),
            base64: jpeg(&img, 85)?,
            media_type: "image/jpeg",
            mark: mark_shot,
        });
    }
    Ok(shots)
}

fn mark_on(mark: &Mark, region: &Box2, b: &Rect, full: &RgbaImage, size: (u32, u32), index: usize) -> Result<MarkShot, String> {
    let c = crop_rect(region, b, full.width() as f64, full.height() as f64);
    let crop = imageops::crop_imm(full, c.x, c.y, c.width.min(full.width() - c.x), c.height.min(full.height() - c.y)).to_image();
    let crop = fit(&crop, MARK_EDGE);
    // The model gets what was actually drawn (the circle, the line, the point), not the padded crop box.
    let (drawn, kind) = match *mark {
        Mark::Point { x, y, .. } => (Box2 { x1: x, y1: y, x2: x, y2: y }, "point"),
        Mark::Underline { b } => (b, "underline"),
        Mark::Circle { b } => (b, "circle"),
    };
    let bx = to_image(&drawn, b, size.0 as f64, size.1 as f64, 0.0);
    let text = describe(mark, Box2 { x1: bx.x1 as f64, y1: bx.y1 as f64, x2: bx.x2 as f64, y2: bx.y2 as f64 }, index as u32);
    Ok(MarkShot { x1: bx.x1, y1: bx.y1, x2: bx.x2, y2: bx.y2, kind, text, crop_base64: jpeg(&crop, 90)? })
}
