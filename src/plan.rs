//! MSFS `.pln` (AceXML) flight-plan reader plus route math.

use crate::util::{bearing_deg, distance_to_segment_nm, haversine_nm};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Waypoint {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
    pub kind: String,
}

#[derive(Clone, Debug, Default)]
pub struct FlightPlan {
    pub title: String,
    pub origin: String,
    pub dest: String,
    pub origin_pos: Option<(f64, f64)>,
    pub dest_pos: Option<(f64, f64)>,
    pub dest_elev_ft: f64,
    pub cruise_ft: f64,
    pub route: Vec<Waypoint>,
    pub path: Option<PathBuf>,
    /// Where the plan came from (file path or "SimBrief · CALLSIGN").
    pub source: String,
}

fn tag_text<'a>(chunk: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = chunk.find(&open)? + open.len();
    let rest = &chunk[start..];
    let end = rest.find(&close)?;
    Some(rest[..end].trim())
}

/// Parses "N41° 48' 15.60\"" or "+000013.00" into decimal degrees.
fn dms_to_deg(token: &str) -> Option<f64> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let mut sign = 1.0;
    let mut body = token;
    match token.chars().next()? {
        'N' | 'n' | 'E' | 'e' => body = &token[1..],
        'S' | 's' | 'W' | 'w' => {
            sign = -1.0;
            body = &token[1..];
        }
        '+' => body = &token[1..],
        '-' => {
            sign = -1.0;
            body = &token[1..];
        }
        _ => {}
    }

    let mut parts: Vec<f64> = Vec::new();
    let mut current = String::new();
    for ch in body.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            current.push(ch);
        } else if !current.is_empty() {
            if let Ok(v) = current.parse::<f64>() {
                parts.push(v);
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        if let Ok(v) = current.parse::<f64>() {
            parts.push(v);
        }
    }
    if parts.is_empty() {
        return None;
    }
    let deg = parts[0];
    let min = parts.get(1).copied().unwrap_or(0.0);
    let sec = parts.get(2).copied().unwrap_or(0.0);
    Some(sign * (deg + min / 60.0 + sec / 3600.0))
}

/// Parses "N41° 48' 15.60\",E12° 15' 9.00\",+000013.00".
fn parse_lla(text: &str) -> Option<(f64, f64, f64)> {
    let mut it = text.split(',');
    let lat = dms_to_deg(it.next()?)?;
    let lon = dms_to_deg(it.next()?)?;
    let alt = it.next().and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0);
    Some((lat, lon, alt))
}

impl FlightPlan {
    pub fn parse(text: &str, path: Option<PathBuf>) -> Result<FlightPlan, String> {
        if !text.contains("ATCWaypoint") && !text.contains("FlightPlan") {
            return Err("Not an MSFS .pln flight plan".to_string());
        }

        let source = path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let mut plan = FlightPlan {
            path,
            source,
            title: tag_text(text, "Title").unwrap_or_default().to_string(),
            origin: tag_text(text, "DepartureID")
                .unwrap_or_default()
                .trim()
                .to_string(),
            dest: tag_text(text, "DestinationID")
                .unwrap_or_default()
                .trim()
                .to_string(),
            cruise_ft: tag_text(text, "CruisingAlt")
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(f64::NAN),
            ..Default::default()
        };

        if let Some(lla) = tag_text(text, "DepartureLLA").and_then(parse_lla) {
            plan.origin_pos = Some((lla.0, lla.1));
        }
        if let Some(lla) = tag_text(text, "DestinationLLA").and_then(parse_lla) {
            plan.dest_pos = Some((lla.0, lla.1));
            plan.dest_elev_ft = lla.2;
        }

        // Split on waypoint blocks and pull out id / position / type.
        let mut rest = text;
        while let Some(start) = rest.find("<ATCWaypoint") {
            let block = &rest[start..];
            let block_end = block.find("</ATCWaypoint>").map(|e| e + 14).unwrap_or(block.len());
            let chunk = &block[..block_end.min(block.len())];

            let id = chunk
                .find("id=\"")
                .and_then(|p| {
                    let after = &chunk[p + 4..];
                    after.find('"').map(|q| after[..q].to_string())
                })
                .unwrap_or_default();

            if let Some(pos) = tag_text(chunk, "WorldPosition").and_then(parse_lla) {
                let kind = tag_text(chunk, "ATCWaypointType")
                    .unwrap_or("Waypoint")
                    .to_string();
                let label = tag_text(chunk, "ICAOIdent")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| id.clone());
                plan.route.push(Waypoint {
                    id: label,
                    lat: pos.0,
                    lon: pos.1,
                    kind,
                });
            }

            rest = &rest[start + block_end..];
            if rest.len() == block.len() {
                break;
            }
        }

        if plan.origin_pos.is_none() {
            plan.origin_pos = plan.route.first().map(|w| (w.lat, w.lon));
        }
        if plan.dest_pos.is_none() {
            plan.dest_pos = plan.route.last().map(|w| (w.lat, w.lon));
        }
        if plan.origin.is_empty() {
            plan.origin = plan
                .route
                .first()
                .map(|w| w.id.clone())
                .unwrap_or_else(|| "ORIG".to_string());
        }
        if plan.dest.is_empty() {
            plan.dest = plan
                .route
                .last()
                .map(|w| w.id.clone())
                .unwrap_or_else(|| "DEST".to_string());
        }

        if plan.route.is_empty() && plan.origin_pos.is_none() {
            return Err("No waypoints found in the flight plan".to_string());
        }
        Ok(plan)
    }

    /// Every point of the route, departure and destination included.
    pub fn points(&self) -> Vec<(f64, f64)> {
        let mut pts = Vec::with_capacity(self.route.len() + 2);
        if let Some(p) = self.origin_pos {
            pts.push(p);
        }
        for w in &self.route {
            let last = pts.last().copied();
            if last.map(|l| haversine_nm(l.0, l.1, w.lat, w.lon) > 0.05).unwrap_or(true) {
                pts.push((w.lat, w.lon));
            }
        }
        if let Some(p) = self.dest_pos {
            let last = pts.last().copied();
            if last.map(|l| haversine_nm(l.0, l.1, p.0, p.1) > 0.05).unwrap_or(true) {
                pts.push(p);
            }
        }
        pts
    }

    pub fn total_nm(&self) -> f64 {
        let pts = self.points();
        pts.windows(2)
            .map(|w| haversine_nm(w[0].0, w[0].1, w[1].0, w[1].1))
            .sum()
    }

    /// Distance still to run along the route from the current position.
    pub fn remaining_nm(&self, lat: f64, lon: f64) -> Option<f64> {
        let pts = self.points();
        if pts.len() < 2 {
            return None;
        }
        let mut best = (f64::MAX, 0usize);
        for i in 0..pts.len() - 1 {
            let d = distance_to_segment_nm(lat, lon, pts[i], pts[i + 1]);
            if d < best.0 {
                best = (d, i);
            }
        }
        let i = best.1;
        let mut remaining = haversine_nm(lat, lon, pts[i + 1].0, pts[i + 1].1);
        for j in i + 1..pts.len() - 1 {
            remaining += haversine_nm(pts[j].0, pts[j].1, pts[j + 1].0, pts[j + 1].1);
        }
        Some(remaining)
    }

    /// Bearing from the current position to the active destination.
    pub fn bearing_to_dest(&self, lat: f64, lon: f64) -> Option<f64> {
        let d = self.dest_pos?;
        Some(bearing_deg(lat, lon, d.0, d.1))
    }

    /// Next waypoint still ahead of the aircraft (nearest one plus one).
    pub fn next_waypoint(&self, lat: f64, lon: f64) -> Option<&Waypoint> {
        if self.route.is_empty() {
            return None;
        }
        let mut best = (f64::MAX, 0usize);
        for (i, w) in self.route.iter().enumerate() {
            let d = haversine_nm(lat, lon, w.lat, w.lon);
            if d < best.0 {
                best = (d, i);
            }
        }
        if best.0 < 4.0 && best.1 + 1 < self.route.len() {
            self.route.get(best.1 + 1)
        } else {
            self.route.get(best.1)
        }
    }

    /// 3° descent: track miles needed from the current altitude.
    pub fn tod_nm(&self, alt_ft: f64) -> f64 {
        let ground = if self.dest_elev_ft.is_finite() {
            self.dest_elev_ft
        } else {
            0.0
        };
        ((alt_ft - ground).max(0.0) / 1000.0) * 3.0
    }

    /// Vertical speed that holds a 3° path at the given ground speed.
    pub fn path_vs_fpm(&self, gs_kt: f64) -> f64 {
        -(gs_kt * 5.307).max(0.0)
    }
}

pub fn read_plan(path: &std::path::Path) -> Result<FlightPlan, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read file: {e}"))?;
    FlightPlan::parse(&text, Some(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<SimBase.Document Type="AceXML" version="1,0">
<FlightPlan.FlightPlan>
<Title>LIRF to LIRN</Title>
<FPType>IFR</FPType>
<CruisingAlt>35000</CruisingAlt>
<DepartureID>LIRF</DepartureID>
<DepartureLLA>N41° 48' 15.60",E12° 15' 9.00",+000013.00</DepartureLLA>
<DestinationID>LIRN</DestinationID>
<DestinationLLA>N40° 53' 9.00",E14° 17' 27.00",+0000294.00</DestinationLLA>
<ATCWaypoint id="LIRF">
<ATCWaypointType>Airport</ATCWaypointType>
<WorldPosition>N41° 48' 15.60",E12° 15' 9.00",+000013.00</WorldPosition>
<ICAO><ICAOIdent>LIRF</ICAOIdent></ICAO>
</ATCWaypoint>
<ATCWaypoint id="ELB">
<ATCWaypointType>VOR</ATCWaypointType>
<WorldPosition>N42° 44' 0.00",E10° 17' 0.00",+0000000.00</WorldPosition>
</ATCWaypoint>
<ATCWaypoint id="LIRN">
<ATCWaypointType>Airport</ATCWaypointType>
<WorldPosition>N40° 53' 9.00",E14° 17' 27.00",+0000294.00</WorldPosition>
<ICAO><ICAOIdent>LIRN</ICAOIdent></ICAO>
</ATCWaypoint>
</FlightPlan.FlightPlan>
</SimBase.Document>"#;

    #[test]
    fn parses_plan() {
        let p = FlightPlan::parse(SAMPLE, None).unwrap();
        assert_eq!(p.origin, "LIRF");
        assert_eq!(p.dest, "LIRN");
        assert_eq!(p.route.len(), 3);
        assert!((p.origin_pos.unwrap().0 - 41.8043).abs() < 0.001);
        assert!(p.total_nm() > 150.0 && p.total_nm() < 400.0);
        let rem = p.remaining_nm(41.8043, 12.2525).unwrap();
        assert!(rem > 100.0);
        assert!(p.tod_nm(35000.0) > 100.0);
    }
}
