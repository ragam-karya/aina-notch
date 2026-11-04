// The laptop's position from Windows' location service (Windows.Devices.Geolocation): GPS, Wi-Fi or
// cell, whatever Windows has. It works only while location is on in Windows' privacy settings (and
// allowed for desktop apps); otherwise the caller says how to turn it on. Nothing is kept here.

use std::time::Duration;

/// Why no position: "denied" (location is off or not allowed for desktop apps), "timeout", or other.
#[derive(Debug, Clone, PartialEq)]
pub enum GeoError {
    Denied,
    Timeout,
    Other(String),
}

/// (latitude, longitude), waiting at most `limit` for Windows to answer.
pub fn position(limit: Duration) -> Result<(f64, f64), GeoError> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read());
    });
    rx.recv_timeout(limit).unwrap_or(Err(GeoError::Timeout))
}

#[cfg(windows)]
fn read() -> Result<(f64, f64), GeoError> {
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy};
    let other = |e: windows::core::Error| GeoError::Other(e.message().to_string());
    let access = Geolocator::RequestAccessAsync().map_err(other)?.join().map_err(other)?;
    if access != GeolocationAccessStatus::Allowed {
        return Err(GeoError::Denied);
    }
    let g = Geolocator::new().map_err(other)?;
    let _ = g.SetDesiredAccuracy(PositionAccuracy::Default);
    let pos = g.GetGeopositionAsync().map_err(|e| if e.code().0 as u32 == 0x80070005 { GeoError::Denied } else { other(e) })?.join().map_err(|e| if e.code().0 as u32 == 0x80070005 { GeoError::Denied } else { other(e) })?;
    let p = pos.Coordinate().map_err(other)?.Point().map_err(other)?.Position().map_err(other)?;
    Ok((p.Latitude, p.Longitude))
}

#[cfg(not(windows))]
fn read() -> Result<(f64, f64), GeoError> {
    Err(GeoError::Other("no location service".into()))
}
