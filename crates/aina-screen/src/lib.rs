// aina's screen: what the model sees (captures per display, resized and encoded), and the math that
// maps between screenshot pixels, the overlay space and physical desktop pixels.

pub mod coords;
pub mod region;

#[cfg(windows)]
pub mod capture;
#[cfg(windows)]
pub mod grab;

/// A rectangle in the overlay coordinate space (logical px; what v1 called global DIP).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

/// A display: where it is in physical desktop px, its scale, and its rect in the overlay space.
#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    pub id: String,
    pub phys: (i32, i32, u32, u32),
    pub scale: f64,
    pub bounds: Rect,
    pub primary: bool,
}
