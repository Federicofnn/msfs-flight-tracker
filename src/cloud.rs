//! Logbook sync through the tracker's own account server.
//!
//! Every pilot has an account with an email and a password; the server keeps
//! the logbook and the flight in progress, so the website shows them after the
//! same login. Only a session token is stored on this PC, never the password.

use crate::flight::FlightRecord;
use crate::net;
use crate::util::now_epoch;

/// Where the accounts live: a Cloudflare Worker with its own database.
pub const API: &str = "https://msfs-flight-tracker-account-1vxd6r.freebuff.site/api";
const MAX_TRACK: usize = 400;

pub struct Session {
    pub token: String,
    pub email: String,
}

fn json_headers(token: &str) -> Vec<(&'static str, String)> {
    let mut headers = vec![("Content-Type", "application/json".to_string())];
    if !token.trim().is_empty() {
        headers.push(("Authorization", format!("Bearer {}", token.trim())));
    }
    headers
}

/// A server answer as something the pilot can act on.
fn explain(status: u32, text: &str) -> String {
    let code = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|json| json["error"].as_str().map(|value| value.to_string()))
        .unwrap_or_default();
    match code.as_str() {
        "email_taken" => "An account with this email already exists: sign in instead".to_string(),
        "bad_credentials" => "Email or password is wrong".to_string(),
        "bad_email" => "That email address is not valid".to_string(),
        "short_password" => "The password needs at least 8 characters".to_string(),
        "too_many" => "Too many attempts: wait a few minutes".to_string(),
        "unauthorized" => "Session expired: sign in again".to_string(),
        "too_large" => "The logbook is too big for the server".to_string(),
        _ if status == 0 => "No answer from the server".to_string(),
        _ => format!("The server answered HTTP {status}"),
    }
}

fn session(kind: &str, email: &str, password: &str) -> Result<Session, String> {
    let email = email.trim();
    if email.is_empty() || !email.contains('@') {
        return Err("Write your email first".to_string());
    }
    if password.chars().count() < 8 {
        return Err("The password needs at least 8 characters".to_string());
    }
    let body = serde_json::json!({ "email": email, "password": password }).to_string();
    let (status, text) = net::api_request(
        "POST",
        &format!("{API}/{kind}"),
        &json_headers(""),
        Some(&body),
    )?;
    if !(200..300).contains(&status) {
        return Err(explain(status, &text));
    }
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Malformed answer: {e}"))?;
    let token = json["token"]
        .as_str()
        .ok_or_else(|| "The server did not return a session".to_string())?
        .to_string();
    let email = json["email"].as_str().unwrap_or(email).to_string();
    Ok(Session { token, email })
}

/// Creates the account and signs in at once.
pub fn signup(email: &str, password: &str) -> Result<Session, String> {
    session("signup", email, password)
}

pub fn login(email: &str, password: &str) -> Result<Session, String> {
    session("login", email, password)
}

/// Forgets the session on the server; the local token is dropped by the app.
pub fn logout(token: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        return Ok(());
    }
    let (status, text) = net::api_request(
        "POST",
        &format!("{API}/logout"),
        &json_headers(token),
        Some("{}"),
    )?;
    if (200..300).contains(&status) || status == 401 {
        return Ok(());
    }
    Err(explain(status, &text))
}

/// A copy that keeps the upload small: the flown track is trimmed, the totals
/// and the landing are kept.
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

pub fn payload(flights: &[FlightRecord], live: Option<&serde_json::Value>) -> String {
    let mut json = serde_json::json!({
        "app": "MSFS Flight Tracker",
        "version": crate::update::VERSION,
        "updated": now_epoch(),
        "flights": for_cloud(flights),
    });
    // The flight in progress, so the website can follow it from anywhere.
    if let Some(live) = live {
        json["live"] = live.clone();
    }
    json.to_string()
}

/// Uploads the whole logbook and the flight in progress.
pub fn push(
    token: &str,
    flights: &[FlightRecord],
    live: Option<&serde_json::Value>,
) -> Result<usize, String> {
    if token.trim().is_empty() {
        return Err("Sign in to your account first".to_string());
    }
    let body = payload(flights, live);
    let (status, text) = net::api_request(
        "PUT",
        &format!("{API}/logbook"),
        &json_headers(token),
        Some(&body),
    )?;
    if !(200..300).contains(&status) {
        return Err(explain(status, &text));
    }
    Ok(flights.len())
}

/// Downloads the flights stored in the account.
pub fn pull(token: &str) -> Result<Vec<FlightRecord>, String> {
    if token.trim().is_empty() {
        return Err("Sign in to your account first".to_string());
    }
    let (status, text) = net::api_request(
        "GET",
        &format!("{API}/logbook"),
        &json_headers(token),
        None,
    )?;
    if !(200..300).contains(&status) {
        return Err(explain(status, &text));
    }
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Malformed logbook: {e}"))?;
    serde_json::from_value::<Vec<FlightRecord>>(json["flights"].clone())
        .map_err(|e| format!("The account does not contain a logbook: {e}"))
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
        let text = payload(&[record("a", 5)], None);
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["flights"][0]["id"], "a");
        assert!(json["version"].is_string());
        assert!(json.get("live").is_none());
    }

    #[test]
    fn the_live_flight_travels_with_the_logbook() {
        let live = serde_json::json!({ "callsign": "ITY610", "eta": 1_766_000_000u64 });
        let text = payload(&[], Some(&live));
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["live"]["callsign"], "ITY610");
        assert_eq!(json["flights"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn server_errors_become_sentences() {
        let body = r#"{"error":"bad_credentials","message":"Email or password is wrong"}"#;
        assert_eq!(explain(401, body), "Email or password is wrong");
        let taken = r#"{"error":"email_taken"}"#;
        assert!(explain(409, taken).contains("already exists"));
        assert_eq!(explain(0, ""), "No answer from the server");
    }

    #[test]
    #[ignore]
    fn account_roundtrip() {
        let email = format!("app-test-{}@example.com", now_epoch());
        let password = "prova12345";
        let session = signup(&email, password).expect("create account");
        assert_eq!(session.email, email);
        let mut flight = record("roundtrip-1", 1_700_000_000);
        flight.origin = "LIRF".to_string();
        flight.dest = "LIRN".to_string();
        flight.duration_s = 3600.0;
        assert_eq!(push(&session.token, &[flight], None).expect("push"), 1);
        let pulled = pull(&session.token).expect("pull");
        assert_eq!(pulled.len(), 1);
        assert_eq!(pulled[0].origin, "LIRF");
        assert_eq!(login(&email, password).expect("login").email, email);
        logout(&session.token).expect("logout");
        assert!(pull(&session.token).is_err(), "the session must be gone");
        // The test account is deleted, so the server stays clean.
        let again = login(&email, password).expect("login again");
        let (status, _) = net::api_request(
            "DELETE",
            &format!("{API}/account"),
            &json_headers(&again.token),
            None,
        )
        .expect("delete");
        assert!((200..300).contains(&status), "delete answered {status}");
    }

    #[test]
    fn a_full_track_is_trimmed_for_the_upload() {
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
