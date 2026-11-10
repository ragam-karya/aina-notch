// Saves a region of the screen as PNG, for demos (AINA_DEMO_OUT), like v1's capturePage of the top
// of the overlay. GDI copy of the composed desktop, so the overlay must not be capture-protected
// (demos turn protection off). Screenshots for Claude use aina-screen instead.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SRCCOPY, SelectObject,
};

/// Physical desktop pixels → PNG file.
pub fn region_png(x: i32, y: i32, w: i32, h: i32, path: &Path) -> Result<(), String> {
    if w <= 0 || h <= 0 {
        return Err("empty region".into());
    }
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let copied = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = GetDIBits(mem, bmp, 0, h as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        copied.map_err(|e| e.to_string())?;
        if lines == 0 {
            return Err("GetDIBits failed".into());
        }
    }
    // BGRA → RGBA, opaque.
    for px in pixels.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
        px[3] = 255;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut wr| wr.write_image_data(&pixels)).map_err(|e| e.to_string())
}
