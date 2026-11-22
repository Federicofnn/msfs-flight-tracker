//! Small, dependency-free helpers: geodesy, formatting, UTC timestamps.

const EARTH_RADIUS_NM: f64 = 3440.065;

/// Great-circle distance in nautical miles.
pub fn haversine_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let p1 = lat1.to_radians();
    let p2 = lat2.to_radians();
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_NM * a.sqrt().clamp(-1.0, 1.0).asin()
}

/// Initial great-circle bearing in degrees (0..360).
pub fn bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let p1 = lat1.to_radians();
    let p2 = lat2.to_radians();
    let dl = (lon2 - lon1).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

/// Distance in NM from a point to a great-circle segment (flat approximation,
/// good to a fraction of a mile over the distances a flight plan uses).
pub fn distance_to_segment_nm(lat: f64, lon: f64, a: (f64, f64), b: (f64, f64)) -> f64 {
    let k = lat.to_radians().cos();
    let ax = (a.1 - lon) * k * 60.0;
    let ay = (a.0 - lat) * 60.0;
    let bx = (b.1 - lon) * k * 60.0;
    let by = (b.0 - lat) * 60.0;
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-9 {
        return (ax * ax + ay * ay).sqrt();
    }
    let t = (-(ax * dx + ay * dy) / len2).clamp(0.0, 1.0);
    let (px, py) = (ax + dx * t, ay + dy * t);
    (px * px + py * py).sqrt()
}

/// "1h 24m", "12m 30s", "45s"
pub fn fmt_duration(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "—".to_string();
    }
    let s = secs.round() as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let ss = s % 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {ss:02}s")
    } else {
        format!("{ss}s")
    }
}

/// "01:23:45" or "23:45" for short spans.
pub fn fmt_clock(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "--:--".to_string();
    }
    let s = secs.round() as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let ss = s % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{ss:02}")
    } else {
        format!("{m:02}:{ss:02}")
    }
}

/// Number with a fixed number of decimals, or an em dash when unavailable.
pub fn num(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        "—".to_string()
    } else {
        format!("{:.*}", decimals, v)
    }
}

pub fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// "22 Nov 2026 09:41Z"
pub fn fmt_utc(epoch: u64) -> String {
    let days = (epoch / 86_400) as i64;
    let secs = epoch % 86_400;
    let (y, m, d) = civil_from_days(days);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{:02} {} {} {:02}:{:02}Z",
        d,
        MONTHS[(m as usize).clamp(1, 12) - 1],
        y,
        secs / 3600,
        (secs % 3600) / 60
    )
}

/// "2026-11-22 09:41:07Z"
pub fn fmt_utc_full(epoch: u64) -> String {
    let days = (epoch / 86_400) as i64;
    let secs = epoch % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// "09:41Z" — wall clock for ETA chips.
pub fn fmt_zulu_hhmm(epoch: u64) -> String {
    let secs = epoch % 86_400;
    format!("{:02}:{:02}Z", secs / 3600, (secs % 3600) / 60)
}
