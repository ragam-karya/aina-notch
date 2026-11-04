// Mapping a marked region (overlay space, from gesture.rs) to what the model sees: its box in
// screenshot pixels, and a padded crop of the full-resolution capture so small text inside it stays
// readable. Pure. Port of src/region.js.

use badi_input::gesture::Box2;

use crate::Rect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IBox {
    pub x1: i64,
    pub y1: i64,
    pub x2: i64,
    pub y2: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

// JS Math.round: halves go up.
fn round(v: f64) -> i64 {
    (v + 0.5).floor() as i64
}

/// The box in an image of size (width, height) that covers display `bounds`, clamped to the image.
pub fn to_image(b: &Box2, bounds: &Rect, width: f64, height: f64, pad: f64) -> IBox {
    let (sx, sy) = (width / bounds.width, height / bounds.height);
    let (px, py) = ((b.x2 - b.x1) * pad, (b.y2 - b.y1) * pad);
    IBox {
        x1: round(((b.x1 - px - bounds.x) * sx).clamp(0.0, width)),
        y1: round(((b.y1 - py - bounds.y) * sy).clamp(0.0, height)),
        x2: round(((b.x2 + px - bounds.x) * sx).clamp(0.0, width)),
        y2: round(((b.y2 + py - bounds.y) * sy).clamp(0.0, height)),
    }
}

/// Crop rectangle in the full-resolution capture, with 15% context around the box.
pub fn crop_rect(b: &Box2, bounds: &Rect, width: f64, height: f64) -> Crop {
    let r = to_image(b, bounds, width, height, 0.15);
    Crop { x: r.x1 as u32, y: r.y1 as u32, width: (r.x2 - r.x1).max(1) as u32, height: (r.y2 - r.y1).max(1) as u32 }
}

// Same case as test/region.test.js.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_screenshot_pixels_on_a_second_display_and_crops_with_context() {
        let bounds = Rect { x: 1920.0, y: 0.0, width: 1920.0, height: 1080.0 };
        // Screenshot scaled to 1280x720: a 192x108 box at (2112,108) becomes 128x72 at (128,72).
        let b = Box2 { x1: 2112.0, y1: 108.0, x2: 2304.0, y2: 216.0 };
        assert_eq!(to_image(&b, &bounds, 1280.0, 720.0, 0.0), IBox { x1: 128, y1: 72, x2: 256, y2: 144 });
        // Full-resolution 3840x2160 capture (scale 2): 15% padding on each side, clamped to the image.
        let b = Box2 { x1: 1920.0, y1: 0.0, x2: 2120.0, y2: 100.0 };
        assert_eq!(crop_rect(&b, &bounds, 3840.0, 2160.0), Crop { x: 0, y: 0, width: 460, height: 230 });
    }
}
