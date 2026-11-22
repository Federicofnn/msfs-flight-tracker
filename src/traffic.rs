//! Live VATSIM and IVAO traffic.
//!
//! Both networks publish one big JSON document. Parsing the whole thing would
//! cost tens of megabytes, so the feed is scanned object by object and only the
//! fields the map needs are copied out.

use crate::net;
use std::time::Instant;

pub const VATSIM: u8 = 0;
pub const IVAO: u8 = 1;

const VATSIM_URL: &str = "https://data.vatsim.net/v3/vatsim-data.json";
const IVAO_URL: &str = "https://api.ivao.aero/v2/tracker/whazzup";

#[derive(Clone, Debug)]
pub struct Pilot {
    pub callsign: String,
    pub lat: f64,
    pub lon: f64,
    pub alt_ft: f64,
    pub gs_kt: f64,
    pub hdg: f32,
    pub on_ground: bool,
    pub aircraft: String,
    pub network: u8,
}

impl Pilot {
    pub fn network_name(&self) -> &'static str {
        if self.network == IVAO {
            "IVAO"
        } else {
            "VATSIM"
        }
    }
}

/// Small copy handed to the map renderer.
#[derive(Clone, Copy)]
pub struct Blip {
    pub lat: f32,
    pub lon: f32,
    pub hdg: f32,
    pub on_ground: bool,
    pub network: u8,
}

impl From<&Pilot> for Blip {
    fn from(pilot: &Pilot) -> Self {
        Self {
            lat: pilot.lat as f32,
            lon: pilot.lon as f32,
            hdg: pilot.hdg,
            on_ground: pilot.on_ground,
            network: pilot.network,
        }
    }
}

/// Filter state, shared by the map and the traffic panel.
#[derive(Clone)]
pub struct Filter {
    pub vatsim: bool,
    pub ivao: bool,
    pub callsign: String,
    pub aircraft: String,
    /// 0 = all, 1 = airborne, 2 = on ground.
    pub status: u8,
    /// 0 = all, 1 = below 10k, 2 = 10k to 30k, 3 = above 30k.
    pub altitude: u8,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            vatsim: true,
            ivao: true,
            callsign: String::new(),
            aircraft: String::new(),
            status: 0,
            altitude: 0,
        }
    }
}

impl Filter {
    pub fn active(&self) -> bool {
        self.vatsim || self.ivao
    }

    pub fn matches(&self, pilot: &Pilot) -> bool {
        if pilot.network == IVAO && !self.ivao {
            return false;
        }
        if pilot.network == VATSIM && !self.vatsim {
            return false;
        }
        let callsign = self.callsign.trim().to_ascii_uppercase();
        if !callsign.is_empty() && !pilot.callsign.to_ascii_uppercase().contains(&callsign) {
            return false;
        }
        let aircraft = self.aircraft.trim().to_ascii_uppercase();
        if !aircraft.is_empty() && !pilot.aircraft.to_ascii_uppercase().contains(&aircraft) {
            return false;
        }
        match self.status {
            1 if pilot.on_ground || pilot.alt_ft < 200.0 => return false,
            2 if !pilot.on_ground && pilot.alt_ft >= 200.0 => return false,
            _ => {}
        }
        match self.altitude {
            1 if pilot.alt_ft >= 10_000.0 => return false,
            2 if !(10_000.0..30_000.0).contains(&pilot.alt_ft) => return false,
            3 if pilot.alt_ft < 30_000.0 => return false,
            _ => {}
        }
        true
    }
}

#[derive(Default)]
pub struct Traffic {
    pub pilots: Vec<Pilot>,
    pub vatsim_total: usize,
    pub ivao_total: usize,
    pub vatsim_error: String,
    pub ivao_error: String,
    pub updated: Option<Instant>,
    pub loading: bool,
}

impl Traffic {
    pub fn visible(&self, filter: &Filter) -> Vec<Blip> {
        self.pilots
            .iter()
            .filter(|pilot| filter.matches(pilot))
            .map(Blip::from)
            .collect()
    }

    pub fn count(&self, filter: &Filter) -> usize {
        self.pilots.iter().filter(|pilot| filter.matches(pilot)).count()
    }

    pub fn reset(&mut self) {
        self.pilots.clear();
        self.vatsim_total = 0;
        self.ivao_total = 0;
        self.vatsim_error.clear();
        self.ivao_error.clear();
        self.updated = None;
    }

    pub fn status_line(&self) -> String {
        match self.updated {
            Some(at) => {
                let seconds = at.elapsed().as_secs();
                if seconds < 60 {
                    format!("updated {seconds}s ago")
                } else {
                    format!("updated {}m ago", seconds / 60)
                }
            }
            None if self.loading => "loading feeds…".to_string(),
            None => "not loaded".to_string(),
        }
    }
}

/// What one refresh produced, for each network.
pub struct Fetched {
    pub vatsim: Result<Vec<Pilot>, String>,
    pub ivao: Result<Vec<Pilot>, String>,
}

pub fn fetch(filter: &Filter) -> Fetched {
    let vatsim = if filter.vatsim {
        fetch_vatsim()
    } else {
        Ok(Vec::new())
    };
    let ivao = if filter.ivao {
        fetch_ivao()
    } else {
        Ok(Vec::new())
    };
    Fetched { vatsim, ivao }
}

pub fn fetch_vatsim() -> Result<Vec<Pilot>, String> {
    let body = net::http_get_max(VATSIM_URL, 64 * 1024 * 1024)?;
    let text = String::from_utf8_lossy(&body);
    let pilots = parse(&text, VATSIM);
    if pilots.is_empty() {
        return Err("VATSIM feed had no pilots".to_string());
    }
    Ok(pilots)
}

pub fn fetch_ivao() -> Result<Vec<Pilot>, String> {
    let body = net::http_get_max(IVAO_URL, 32 * 1024 * 1024)?;
    let text = String::from_utf8_lossy(&body);
    let pilots = parse(&text, IVAO);
    if pilots.is_empty() {
        return Err("IVAO feed had no pilots".to_string());
    }
    Ok(pilots)
}

/// Scans the `"pilots"` array and keeps one entry per client.
pub fn parse(text: &str, network: u8) -> Vec<Pilot> {
    let mut pilots = Vec::new();
    for object in array_objects(text, "pilots") {
        let Some(callsign) = string_field(object, "callsign") else {
            continue;
        };
        let (Some(lat), Some(lon)) = (number_field(object, "latitude"), number_field(object, "longitude")) else {
            continue;
        };
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            continue;
        }
        let alt_ft = number_field(object, "altitude").unwrap_or(0.0).max(0.0);
        let gs_kt = number_field(object, "groundSpeed")
            .or_else(|| number_field(object, "groundspeed"))
            .unwrap_or(0.0)
            .max(0.0);
        let hdg = number_field(object, "heading").unwrap_or(0.0) as f32;
        let on_ground = bool_field(object, "onGround")
            .unwrap_or(alt_ft < 100.0 && gs_kt < 40.0);
        let aircraft = string_field(object, "aircraft_short")
            .or_else(|| string_field(object, "aircraftId"))
            .or_else(|| string_field(object, "aircraft"))
            .unwrap_or_default();
        pilots.push(Pilot {
            callsign,
            lat,
            lon,
            alt_ft,
            gs_kt,
            hdg,
            on_ground,
            aircraft,
            network,
        });
    }
    pilots
}

/// Returns every `{...}` directly inside the array that follows `"key":[`.
fn array_objects<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    let needle = format!("\"{key}\":[");
    let Some(start) = text.find(&needle) else {
        return Vec::new();
    };
    let bytes = text.as_bytes();
    let mut objects = Vec::new();
    let mut i = start + needle.len();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut object_start = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'{' => {
                    if depth == 0 {
                        object_start = i;
                    }
                    depth += 1;
                }
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        objects.push(&text[object_start..=i]);
                    }
                }
                b']' if depth == 0 => break,
                _ => {}
            }
        }
        i += 1;
    }
    objects
}

fn field_offset(object: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\":");
    object.find(&needle).map(|at| at + needle.len())
}

fn number_field(object: &str, key: &str) -> Option<f64> {
    let rest = &object[field_offset(object, key)?..];
    let rest = rest.trim_start();
    let end = rest
        .find(|c: char| c != '-' && c != '+' && c != '.' && !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn bool_field(object: &str, key: &str) -> Option<bool> {
    let rest = &object[field_offset(object, key)?..];
    let rest = rest.trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn string_field(object: &str, key: &str) -> Option<String> {
    let rest = &object[field_offset(object, key)?..];
    let rest = rest.trim_start();
    let mut chars = rest.chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for c in chars {
        if escaped {
            match c {
                'n' | 't' | 'r' => out.push(' '),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'u' => {}
                other => out.push(other),
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(out.trim().to_string());
        } else {
            out.push(c);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_vatsim_pilots() {
        let feed = r#"{"pilots":[{"callsign":"ITY610","latitude":41.8,"longitude":12.25,"altitude":34000,"groundspeed":470,"heading":210,"flight_plan":{"aircraft_short":"A320","departure":"LIRF"}},{"callsign":"DLH1","latitude":50.0,"longitude":8.5,"altitude":0,"groundspeed":0,"heading":90,"flight_plan":null}],"controllers":[]}"#;
        let pilots = parse(feed, VATSIM);
        assert_eq!(pilots.len(), 2);
        assert_eq!(pilots[0].callsign, "ITY610");
        assert_eq!(pilots[0].aircraft, "A320");
        assert_eq!(pilots[0].alt_ft, 34000.0);
        assert_eq!(pilots[0].network_name(), "VATSIM");
        assert!(pilots[1].on_ground);
    }

    #[test]
    fn reads_ivao_nested_tracks() {
        let feed = r#"{"clients":{"pilots":[{"callsign":"BAW5A","lastTrack":{"altitude":33645,"groundSpeed":465,"heading":236,"latitude":50.0215,"longitude":160.00816,"onGround":false},"flightPlan":{"aircraftId":"A359","departureId":"EGLL"}}]}}"#;
        let pilots = parse(feed, IVAO);
        assert_eq!(pilots.len(), 1);
        assert_eq!(pilots[0].aircraft, "A359");
        assert_eq!(pilots[0].hdg, 236.0);
        assert!(!pilots[0].on_ground);
        assert_eq!(pilots[0].network_name(), "IVAO");
    }

    #[test]
    fn filters_narrow_the_view() {
        let pilots = parse(
            r#"{"pilots":[{"callsign":"ITY610","latitude":41.8,"longitude":12.25,"altitude":34000,"groundspeed":470,"heading":210,"flight_plan":{"aircraft_short":"A320"}},{"callsign":"DLH1","latitude":50.0,"longitude":8.5,"altitude":0,"groundspeed":0,"heading":90,"flight_plan":{"aircraft_short":"A20N"}}]}"#,
            VATSIM,
        );
        let mut filter = Filter::default();
        assert_eq!(pilots.iter().filter(|p| filter.matches(p)).count(), 2);
        filter.status = 2;
        assert_eq!(pilots.iter().filter(|p| filter.matches(p)).count(), 1);
        filter.status = 0;
        filter.ivao = false;
        filter.vatsim = false;
        assert_eq!(pilots.iter().filter(|p| filter.matches(p)).count(), 0);
        filter.vatsim = true;
        filter.aircraft = "a320".into();
        assert_eq!(pilots.iter().filter(|p| filter.matches(p)).count(), 1);
    }
}
