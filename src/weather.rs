//! Live METAR and TAF inside the app, straight from the NOAA Aviation Weather
//! Center. The two endpoints are free, need no key and answer in JSON:
//!
//!   /api/data/metar?ids=LIRF,LIRN&format=json
//!   /api/data/taf?ids=LIRF&format=json
//!
//! Nothing opens a browser: the panel decodes the report itself.

use crate::net;
use crate::util::now_epoch;
use serde_json::Value;

const METAR: &str = "https://aviationweather.gov/api/data/metar";
const TAF: &str = "https://aviationweather.gov/api/data/taf";

#[derive(Clone, Debug, Default)]
pub struct Airport {
    pub icao: String,
    pub name: String,
    /// Raw observation, exactly as the station reports it.
    pub metar: String,
    pub observed: u64,
    pub taf: String,
    pub taf_from: u64,
    pub taf_to: u64,
    pub wind: String,
    pub visibility: String,
    pub clouds: String,
    pub temperature: String,
    pub qnh: String,
    /// VFR / MVFR / IFR / LIFR.
    pub category: String,
}

#[derive(Clone, Debug, Default)]
pub struct Set {
    pub airports: Vec<Airport>,
    pub fetched: u64,
    pub error: String,
}

impl Set {
    pub fn age_secs(&self) -> u64 {
        if self.fetched == 0 {
            u64::MAX
        } else {
            now_epoch().saturating_sub(self.fetched)
        }
    }
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn number(value: &Value, key: &str) -> Option<f64> {
    match value.get(key) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn wind(item: &Value) -> String {
    let direction = match item.get("wdir") {
        Some(Value::Number(n)) => format!("{:03.0}°", n.as_f64().unwrap_or(0.0)),
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_uppercase(),
        _ => String::new(),
    };
    let speed = number(item, "wspd").unwrap_or(0.0);
    if direction.is_empty() && speed <= 0.5 {
        return "calmo".to_string();
    }
    match number(item, "wgst") {
        Some(gust) if gust > speed => format!("{direction} {speed:.0} G {gust:.0} kt"),
        _ => format!("{direction} {speed:.0} kt"),
    }
}

fn visibility(item: &Value) -> String {
    let raw = text(item, "visib");
    if raw.is_empty() {
        return "—".to_string();
    }
    if raw.ends_with('+') {
        format!("≥ {} SM", raw.trim_end_matches('+'))
    } else {
        format!("{raw} SM")
    }
}

/// Cloud layers as "FEW045 SCT120", the way a pilot reads them.
fn clouds(item: &Value) -> String {
    let mut layers = Vec::new();
    if let Some(list) = item.get("clouds").and_then(|value| value.as_array()) {
        for layer in list {
            let cover = text(layer, "cover").to_uppercase();
            if cover.is_empty() {
                continue;
            }
            match number(layer, "base") {
                Some(base) => layers.push(format!("{cover}{:03.0}", (base / 100.0).round())),
                None => layers.push(cover),
            }
        }
    }
    if layers.is_empty() {
        text(item, "cover")
    } else {
        layers.join(" ")
    }
}

fn temperature(item: &Value) -> String {
    match (number(item, "temp"), number(item, "dewp")) {
        (Some(temp), Some(dew)) => format!("{temp:.0}° / {dew:.0}°C"),
        (Some(temp), None) => format!("{temp:.0}°C"),
        _ => "—".to_string(),
    }
}

fn pressure(item: &Value) -> String {
    match number(item, "altim") {
        Some(hpa) => format!("{hpa:.0} hPa"),
        None => "—".to_string(),
    }
}

fn parse_metar(item: &Value) -> Airport {
    Airport {
        icao: text(item, "icaoId"),
        name: text(item, "name"),
        metar: text(item, "rawOb"),
        observed: number(item, "obsTime").unwrap_or(0.0).max(0.0) as u64,
        taf: String::new(),
        taf_from: 0,
        taf_to: 0,
        wind: wind(item),
        visibility: visibility(item),
        clouds: clouds(item),
        temperature: temperature(item),
        qnh: pressure(item),
        category: text(item, "fltCat").to_uppercase(),
    }
}

/// The latest observation wins: the API can return a few hours of history.
fn newest_metar(list: &Value, icao: &str) -> Option<Airport> {
    let items = list.as_array()?;
    let mut best: Option<Airport> = None;
    for item in items {
        if !text(item, "icaoId").eq_ignore_ascii_case(icao) {
            continue;
        }
        let parsed = parse_metar(item);
        if parsed.metar.is_empty() {
            continue;
        }
        if best
            .as_ref()
            .map(|current| parsed.observed > current.observed)
            .unwrap_or(true)
        {
            best = Some(parsed);
        }
    }
    best
}

fn newest_taf(list: &Value, icao: &str) -> Option<(String, u64, u64)> {
    let items = list.as_array()?;
    let mut best: Option<(String, u64, u64)> = None;
    for item in items {
        if !text(item, "icaoId").eq_ignore_ascii_case(icao) {
            continue;
        }
        let raw = text(item, "rawTAF");
        if raw.is_empty() {
            continue;
        }
        let from = number(item, "validTimeFrom").unwrap_or(0.0).max(0.0) as u64;
        let to = number(item, "validTimeTo").unwrap_or(0.0).max(0.0) as u64;
        if best.as_ref().map(|(_, f, _)| from > *f).unwrap_or(true) {
            best = Some((raw, from, to));
        }
    }
    best
}

/// Downloads METAR and TAF for every code; a failing endpoint does not hide
/// the other one.
pub fn fetch(codes: &[String]) -> Result<Set, String> {
    let mut list: Vec<String> = Vec::new();
    for code in codes {
        let code = code.trim().to_uppercase();
        if code.len() >= 3 && !list.contains(&code) {
            list.push(code);
        }
        if list.len() >= 8 {
            break;
        }
    }
    if list.is_empty() {
        return Err("Add an airport code (for example LIRF) first".to_string());
    }
    let ids = net::encode_component(&list.join(","));
    let metars = net::get_json(&format!("{METAR}?ids={ids}&format=json&hours=2"));
    let tafs = net::get_json(&format!("{TAF}?ids={ids}&format=json"));
    let mut first_error = String::new();
    if let Err(err) = &metars {
        first_error = err.clone();
    }
    if let Err(err) = &tafs {
        if first_error.is_empty() {
            first_error = err.clone();
        }
    }
    if metars.is_err() && tafs.is_err() {
        return Err(first_error);
    }
    let metars = metars.unwrap_or(Value::Array(Vec::new()));
    let tafs = tafs.unwrap_or(Value::Array(Vec::new()));

    let mut airports = Vec::new();
    for icao in &list {
        let mut airport = newest_metar(&metars, icao).unwrap_or_else(|| Airport {
            icao: icao.clone(),
            name: String::new(),
            metar: String::new(),
            observed: 0,
            taf: String::new(),
            taf_from: 0,
            taf_to: 0,
            wind: "—".to_string(),
            visibility: "—".to_string(),
            clouds: "—".to_string(),
            temperature: "—".to_string(),
            qnh: "—".to_string(),
            category: String::new(),
        });
        if airport.name.is_empty() {
            if let Some(items) = tafs.as_array() {
                for item in items {
                    if text(item, "icaoId").eq_ignore_ascii_case(icao) {
                        airport.name = text(item, "name");
                        break;
                    }
                }
            }
        }
        if let Some((raw, from, to)) = newest_taf(&tafs, icao) {
            airport.taf = raw;
            airport.taf_from = from;
            airport.taf_to = to;
        }
        airports.push(airport);
    }

    Ok(Set {
        airports,
        fetched: now_epoch(),
        error: first_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const METARS: &str = r#"[
      {"icaoId":"LIRF","obsTime":1795555200,"temp":14,"dewp":11,"wdir":120,"wspd":6,
       "visib":"6+","altim":1021,"rawOb":"METAR LIRF 242120Z 12006KT 9999 FEW045 14/11 Q1021",
       "name":"Rome/Fiumicino","clouds":[{"cover":"FEW","base":4500}],"fltCat":"VFR"},
      {"icaoId":"LIRN","obsTime":1795553400,"temp":11,"dewp":9,"wdir":"VRB","wspd":2,
       "visib":"6+","altim":1023,"rawOb":"METAR LIRN 242050Z VRB02KT CAVOK 11/09 Q1023",
       "name":"Naples","clouds":[],"cover":"CAVOK","fltCat":"VFR"},
      {"icaoId":"LIRF","obsTime":1795550000,"temp":9,"dewp":7,"wdir":100,"wspd":4,
       "visib":"2","altim":1020,"rawOb":"OLD","clouds":[{"cover":"BKN","base":900}],"fltCat":"IFR"}
    ]"#;

    fn json() -> Value {
        serde_json::from_str(METARS).unwrap()
    }

    #[test]
    fn the_newest_report_wins() {
        let metars = json();
        let rome = newest_metar(&metars, "LIRF").expect("LIRF");
        assert!(rome.metar.starts_with("METAR LIRF 242120Z"));
        assert_eq!(rome.category, "VFR");
        assert_eq!(rome.clouds, "FEW045");
        assert_eq!(rome.temperature, "14° / 11°C");
        assert_eq!(rome.qnh, "1021 hPa");
    }

    #[test]
    fn variable_wind_and_cavok_are_readable() {
        let naples = newest_metar(&json(), "LIRN").expect("LIRN");
        assert_eq!(naples.wind, "VRB 2 kt");
        assert_eq!(naples.clouds, "CAVOK");
        assert!(naples.visibility.starts_with("≥ 6"));
    }

    #[test]
    fn an_unknown_station_is_not_invented() {
        assert!(newest_metar(&json(), "EGLL").is_none());
    }
}
