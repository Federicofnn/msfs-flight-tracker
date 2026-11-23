//! Logbook sync through a GitHub gist.
//!
//! The pilot's logbook is uploaded to a gist they own, so the same flights are
//! visible in the app and on the companion website. The gist is public: the
//! website reads it with the logbook code alone, no token and no login. Nobody
//! else can find it unless they have that code, and nothing passes through a
//! server of ours.

use crate::flight::FlightRecord;
use crate::net;
use crate::util::now_epoch;

const API: &str = "https://api.github.com";
const FILE: &str = "msfs-logbook.json";
const MAX_TRACK: usize = 400;

fn headers(token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Authorization", format!("Bearer {}", token.trim())),
        ("X-GitHub-Api-Version", "2022-11-28".to_string()),
        ("Content-Type", "application/json".to_string()),
        ("User-Agent", "MSFSFlightTracker".to_string()),
    ]
}

/// A copy that is light enough for a gist: the flown track is trimmed, the
/// totals and the landing are kept.
pub fn for_cloud(flights: &[FlightRecord]) -> Vec<FlightRecord> {
    flights
        .iter()
        .map(|record| {
            let mut copy = record.clone();
            if copy.track.len() > MAX_TRACK {
                let step = copy.track.len() as f64 / MAX_TRACK as f64;
                copy.track = (0..MAX_TRACK)
                    .map(|index| record.track[(index as f64 * step) as usize].clone())
                    .collect();
                if let Some(last) = record.track.last() {
                    copy.track.push(last.clone());
                }
            }
            copy
        })
        .collect()
}

pub fn payload(flights: &[FlightRecord]) -> String {
    serde_json::json!({
        "app": "MSFS Flight Tracker",
        "version": crate::update::VERSION,
        "updated": now_epoch(),
        "flights": for_cloud(flights),
    })
    .to_string()
}

fn explain(status: u32) -> String {
    match status {
        401 => "GitHub rejected the token (401). Create a new one with the gist permission.".to_string(),
        403 => "This token is not allowed to write gists (403). It needs the gist permission.".to_string(),
        404 => "Gist not found (404). Check the gist ID, or leave it empty to create a new one.".to_string(),
        422 => "GitHub refused the logbook (422). The gist may be too large.".to_string(),
        other => format!("GitHub answered HTTP {other}"),
    }
}

/// Uploads the logbook and returns the id of the gist that holds it.
pub fn push(token: &str, gist: &str, flights: &[FlightRecord]) -> Result<String, String> {
    if token.trim().is_empty() {
        return Err("Add a GitHub token first".to_string());
    }
    let content = serde_json::to_string(&payload(flights))
        .map_err(|e| format!("Cannot encode the logbook: {e}"))?;
    let id = gist.trim();
    if id.is_empty() {
        return create(token, &content);
    }
    let url = format!("{API}/gists/{id}");
    let body = format!("{{\"files\":{{\"{FILE}\":{{\"content\":{content}}}}}}}");
    let (status, text) = net::api_request("PATCH", &url, &headers(token), Some(&body))?;
    if !(200..300).contains(&status) {
        return Err(explain(status));
    }
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Malformed GitHub response: {e}"))?;
    if json["public"].as_bool().unwrap_or(false) {
        return gist_id(&json);
    }
    // A gist cannot be switched from secret to public: move the logbook to a new
    // one, otherwise the website could not open it without a token.
    create(token, &content)
}

fn create(token: &str, content: &str) -> Result<String, String> {
    let body = format!(
        "{{\"description\":\"MSFS Flight Tracker logbook\",\"public\":true,\"files\":{{\"{FILE}\":{{\"content\":{content}}}}}}}"
    );
    let (status, text) = net::api_request("POST", &format!("{API}/gists"), &headers(token), Some(&body))?;
    if !(200..300).contains(&status) {
        return Err(explain(status));
    }
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Malformed GitHub response: {e}"))?;
    gist_id(&json)
}

fn gist_id(json: &serde_json::Value) -> Result<String, String> {
    json["id"]
        .as_str()
        .map(|value| value.to_string())
        .ok_or_else(|| "GitHub did not return the gist id".to_string())
}

/// Downloads the flights stored in the gist.
pub fn pull(token: &str, gist: &str) -> Result<Vec<FlightRecord>, String> {
    if token.trim().is_empty() {
        return Err("Add a GitHub token first".to_string());
    }
    let id = gist.trim();
    if id.is_empty() {
        return Err("No gist yet: upload the logbook first".to_string());
    }
    let url = format!("{API}/gists/{id}");
    let (status, text) = net::api_request("GET", &url, &headers(token), None)?;
    if !(200..300).contains(&status) {
        return Err(explain(status));
    }
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Malformed GitHub response: {e}"))?;
    let file = &json["files"][FILE];
    if file.is_null() {
        return Err("This gist has no logbook file".to_string());
    }
    if file["truncated"].as_bool().unwrap_or(false) {
        let raw = file["raw_url"]
            .as_str()
            .ok_or_else(|| "GitHub truncated the logbook and gave no raw link".to_string())?;
        let (status, text) = net::api_request("GET", raw, &headers(token), None)?;
        if !(200..300).contains(&status) {
            return Err(explain(status));
        }
        return parse_flights(&text);
    }
    let content = file["content"].as_str().unwrap_or_default();
    parse_flights(content)
}

fn parse_flights(text: &str) -> Result<Vec<FlightRecord>, String> {
    let json: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("Malformed logbook: {e}"))?;
    let list = if json.is_array() {
        json
    } else {
        json["flights"].clone()
    };
    serde_json::from_value::<Vec<FlightRecord>>(list)
        .map_err(|e| format!("The gist does not contain a logbook: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, started: u64) -> FlightRecord {
        FlightRecord {
            id: id.to_string(),
            started,
            ..Default::default()
        }
    }

    #[test]
    fn payload_is_readable_by_the_website() {
        let text = payload(&[record("a", 5)]);
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["flights"][0]["id"], "a");
        assert!(json["version"].is_string());
    }

    #[test]
    #[ignore]
    fn gist_roundtrip() {
        let token = std::fs::read_to_string("tools/.token")
            .expect("write the token to tools/.token first")
            .trim()
            .to_string();
        let mut flight = record("roundtrip-1", 1_700_000_000);
        flight.origin = "LIRF".to_string();
        flight.dest = "LIRN".to_string();
        flight.duration_s = 3600.0;
        let id = push(&token, "", &[flight.clone()]).expect("create gist");
        println!("created gist {id}");
        let pulled = pull(&token, &id).expect("read gist");
        assert_eq!(pulled.len(), 1);
        assert_eq!(pulled[0].origin, "LIRF");
        let updated = push(&token, &id, &[flight, record("roundtrip-2", 1_700_000_100)])
            .expect("update gist");
        assert_eq!(updated, id);
        assert_eq!(pull(&token, &id).expect("read again").len(), 2);
        let url = format!("{API}/gists/{id}");
        let (status, _) = net::api_request("DELETE", &url, &headers(&token), None).expect("delete");
        println!("deleted gist: HTTP {status}");
    }

    #[test]
    fn a_full_track_is_trimmed_for_the_gist() {
        let mut full = record("a", 1);
        full.track = (0..1000)
            .map(|index| crate::flight::TrackPoint {
                t: index as f64,
                lat: 1.0,
                lon: 2.0,
                alt: 0.0,
                gs: 0.0,
                vs: 0.0,
                fuel: 0.0,
            })
            .collect();
        let light = for_cloud(&[full]);
        assert!(light[0].track.len() <= MAX_TRACK + 1);
    }
}
