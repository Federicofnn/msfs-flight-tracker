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

/// "22 Nov 2026 21:41" on the pilot's own clock.
pub fn fmt_local(epoch: u64) -> String {
    let shifted = (epoch as i64 + local_offset_secs()).max(0) as u64;
    let days = (shifted / 86_400) as i64;
    let secs = shifted % 86_400;
    let (y, m, d) = civil_from_days(days);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{:02} {} {} {:02}:{:02}",
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

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Seconds the computer clock is ahead of UTC, so every time the app shows is
/// the pilot's own wall clock instead of Zulu.
#[cfg(windows)]
pub fn local_offset_secs() -> i64 {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;

    unsafe {
        let mut local: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut local);
        let local_epoch = days_from_civil(local.wYear as i64, local.wMonth as i64, local.wDay as i64)
            * 86_400
            + local.wHour as i64 * 3600
            + local.wMinute as i64 * 60
            + local.wSecond as i64;
        let mut offset = local_epoch - now_epoch() as i64;
        // The two clocks can be sampled across midnight.
        while offset > 50_400 {
            offset -= 86_400;
        }
        while offset < -50_400 {
            offset += 86_400;
        }
        // Whole minutes: the samples are a few milliseconds apart.
        (offset as f64 / 60.0).round() as i64 * 60
    }
}

#[cfg(not(windows))]
pub fn local_offset_secs() -> i64 {
    0
}

/// "24 Nov" on the pilot's own calendar.
pub fn fmt_local_date(epoch: u64) -> String {
    let shifted = (epoch as i64 + local_offset_secs()).max(0) as u64;
    let (_, month, day) = civil_from_days((shifted / 86_400) as i64);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{} {}", day, MONTHS[(month as usize).clamp(1, 12) - 1])
}

/// "22:05" on the pilot's own clock.
pub fn fmt_local_hhmm(epoch: u64) -> String {
    let secs = (epoch as i64 + local_offset_secs()).rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_clock_field_is_in_range() {
        let sample = 1_766_000_000u64;
        let hhmm = fmt_local_hhmm(sample);
        let (hour, minute) = hhmm.split_once(':').expect("HH:MM");
        assert!(hour.parse::<u32>().unwrap() < 24);
        assert!(minute.parse::<u32>().unwrap() < 60);
        assert!((-50_400..=50_400).contains(&local_offset_secs()));
        // The local clock and Zulu agree up to the timezone offset.
        let utc = sample % 86_400;
        let local = (sample as i64 + local_offset_secs()).rem_euclid(86_400);
        assert_eq!((local - utc as i64).rem_euclid(86_400), local_offset_secs().rem_euclid(86_400));
    }
}
