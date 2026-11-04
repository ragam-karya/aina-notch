// Coordinate mapping for the computer toolset. Claude works in the pixel space of the screenshots we
// send; the desktop works in physical pixels. Pure. Port of src/brain/coords.js.

use crate::Rect;

/// Long edge of the screenshots sent to Claude. 1536x864 (1920x1080 at 125%) becomes 1280x720.
pub const SHOT_MAX_EDGE: f64 = 1280.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

// JS Math.round: halves go up.
fn round(v: f64) -> f64 {
    (v + 0.5).floor()
}

pub fn shot_size(bounds: &Rect, max_edge: f64) -> Size {
    let scale = (max_edge / bounds.width.max(bounds.height)).min(1.0);
    Size { width: round(bounds.width * scale), height: round(bounds.height * scale) }
}

pub fn check_point(coord: &[f64], shot: Size, what: &str) -> Result<Point, String> {
    let [x, y] = coord else { return Err(format!("{what} must be [x, y] in screenshot pixels.")) };
    if !x.is_finite() || !y.is_finite() {
        return Err(format!("{what} must be [x, y] in screenshot pixels."));
    }
    if *x < 0.0 || *y < 0.0 || *x >= shot.width || *y >= shot.height {
        return Err(format!("{what} [{x}, {y}] is outside the {}x{} screenshot.", shot.width, shot.height));
    }
    Ok(Point { x: *x, y: *y })
}

/// Screenshot pixel -> overlay space on the controlled display (same math as tags::to_global).
pub fn shot_to_dip(p: Point, shot: Size, bounds: &Rect) -> Point {
    Point { x: bounds.x + p.x * bounds.width / shot.width, y: bounds.y + p.y * bounds.height / shot.height }
}

/// Overlay space -> screenshot pixel (for cursor_position).
pub fn dip_to_shot(p: Point, shot: Size, bounds: &Rect) -> Point {
    Point { x: round((p.x - bounds.x) * shot.width / bounds.width), y: round((p.y - bounds.y) * shot.height / bounds.height) }
}

/// Screenshot pixel -> physical screen pixel, given the display's overlay-space -> physical mapping.
pub fn shot_to_physical(p: Point, shot: Size, bounds: &Rect, dip_to_screen: impl Fn(Point) -> Point) -> (i32, i32) {
    let s = dip_to_screen(shot_to_dip(p, shot, bounds));
    (round(s.x) as i32, round(s.y) as i32)
}

/// A zoom region in screenshot pixels -> a crop rectangle in the full-resolution capture.
pub fn zoom_rect(region: &[f64], shot: Size, full: Size) -> Result<(u32, u32, u32, u32), String> {
    let [x0, y0, x1, y1] = region else { return Err("region must be [x0, y0, x1, y1] in screenshot pixels.".into()) };
    if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
        return Err("region must be [x0, y0, x1, y1] in screenshot pixels.".into());
    }
    let (sx, sy) = (full.width / shot.width, full.height / shot.height);
    let left = x0.min(*x1).max(0.0);
    let top = y0.min(*y1).max(0.0);
    let right = x0.max(*x1).min(shot.width);
    let bottom = y0.max(*y1).min(shot.height);
    if right - left < 4.0 || bottom - top < 4.0 {
        return Err("zoom region is empty or outside the screenshot.".into());
    }
    Ok((round(left * sx) as u32, round(top * sy) as u32, round((right - left) * sx) as u32, round((bottom - top) * sy) as u32))
}

// Same cases as the coordinate tests in test/brain-input.test.js.
#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    #[test]
    fn a_125_percent_1080p_display_maps_to_1280x720_and_back_to_physical() {
        let bounds = Rect { x: 0.0, y: 0.0, width: 1536.0, height: 864.0 };
        let shot = shot_size(&bounds, SHOT_MAX_EDGE);
        assert_eq!(shot, Size { width: 1280.0, height: 720.0 });
        let to_screen = |q: Point| p(q.x * 1.25, q.y * 1.25);
        assert_eq!(shot_to_physical(p(640.0, 360.0), shot, &bounds, to_screen), (960, 540));
        assert_eq!(shot_to_physical(p(1279.0, 719.0), shot, &bounds, to_screen), (1919, 1079));
        assert_eq!(dip_to_shot(p(768.0, 432.0), shot, &bounds), p(640.0, 360.0));
    }

    #[test]
    fn a_second_monitor_keeps_its_offset() {
        let bounds = Rect { x: 1536.0, y: -200.0, width: 1920.0, height: 1080.0 };
        let shot = shot_size(&bounds, SHOT_MAX_EDGE);
        assert_eq!(shot, Size { width: 1280.0, height: 720.0 });
        assert_eq!(shot_to_dip(p(0.0, 0.0), shot, &bounds), p(1536.0, -200.0));
        assert_eq!(shot_to_dip(p(640.0, 360.0), shot, &bounds), p(1536.0 + 960.0, -200.0 + 540.0));
    }

    #[test]
    fn coordinates_outside_the_screenshot_are_rejected() {
        let shot = Size { width: 1280.0, height: 720.0 };
        assert_eq!(check_point(&[10.0, 20.0], shot, "coordinate"), Ok(p(10.0, 20.0)));
        assert!(check_point(&[1280.0, 10.0], shot, "coordinate").unwrap_err().contains("outside the 1280x720 screenshot"));
        assert!(check_point(&[-1.0, 10.0], shot, "coordinate").unwrap_err().contains("outside"));
        assert!(check_point(&[10.0], shot, "coordinate").unwrap_err().contains("must be [x, y]"));
    }

    #[test]
    fn zoom_rect_maps_a_screenshot_region_onto_the_full_capture() {
        let shot = Size { width: 1280.0, height: 720.0 };
        let full = Size { width: 1920.0, height: 1080.0 };
        assert_eq!(zoom_rect(&[100.0, 100.0, 300.0, 200.0], shot, full), Ok((150, 150, 300, 150)));
        assert_eq!(zoom_rect(&[300.0, 200.0, 100.0, 100.0], shot, full), Ok((150, 150, 300, 150)));
        assert_eq!(zoom_rect(&[1200.0, 650.0, 1500.0, 900.0], shot, full), Ok((1800, 975, 120, 105)));
        assert!(zoom_rect(&[5.0, 5.0, 6.0, 6.0], shot, full).unwrap_err().contains("empty"));
    }
}
