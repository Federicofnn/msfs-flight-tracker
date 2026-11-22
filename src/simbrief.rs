//! SimBrief OFP import (latest flight plan of a pilot).

use crate::net;
use crate::plan::{FlightPlan, Waypoint};
use crate::util::now_epoch;
use serde_json::Value;

const LB_TO_KG: f64 = 0.453_592_37;

/// SimBrief only gives the ICAO code; the card wants the airline name.
pub fn airline_name(code: &str) -> String {
    let name = match code.trim().to_ascii_uppercase().as_str() {
        "DLH" => "Lufthansa",
        "UAE" => "Emirates",
        "BAW" => "British Airways",
        "AFR" => "Air France",
        "KLM" => "KLM",
        "RYR" => "Ryanair",
        "EZY" => "easyJet",
        "SWR" => "SWISS",
        "AUA" => "Austrian Airlines",
        "ITY" => "ITA Airways",
        "ITA" => "ITA Airways",
        "THY" => "Turkish Airlines",
        "QTR" => "Qatar Airways",
        "SIA" => "Singapore Airlines",
        "CPA" => "Cathay Pacific",
        "AAL" => "American Airlines",
        "DAL" => "Delta Air Lines",
        "UAL" => "United Airlines",
        "ACA" => "Air Canada",
        "IBE" => "Iberia",
        "VLG" => "Vueling",
        "EIN" => "Aer Lingus",
        "TAP" => "TAP Air Portugal",
        "SAS" => "SAS",
        "NAX" => "Norwegian",
        "WZZ" => "Wizz Air",
        "FIN" => "Finnair",
        "AEE" => "Aegean Airlines",
        "CCA" => "Air China",
        "CES" => "China Eastern",
        "CSN" => "China Southern",
        "ANA" => "All Nippon Airways",
        "JAL" => "Japan Airlines",
        "KAL" => "Korean Air",
        "ETH" => "Ethiopian Airlines",
        "MSR" => "EgyptAir",
        "RAM" => "Royal Air Maroc",
        "SVA" => "Saudia",
        "ETD" => "Etihad Airways",
        "QFA" => "Qantas",
        "ANZ" => "Air New Zealand",
        "LOT" => "LOT Polish Airlines",
        "BTI" => "airBaltic",
        "AIC" => "Air India",
        "JAI" => "Jet Airways",
        "SEJ" => "SpiceJet",
        "IGO" => "IndiGo",
        "KAC" => "Kuwait Airways",
        "GFA" => "Gulf Air",
        "OMA" => "Oman Air",
        "RJA" => "Royal Jordanian",
        "MEA" => "Middle East Airlines",
        "SAA" => "South African Airways",
        "KQA" => "Kenya Airways",
        "TAR" => "Tunisair",
        "AZA" => "Alitalia",
        "AZU" => "Azul",
        "TAM" => "LATAM",
        "ARG" => "Aerolineas Argentinas",
        "AMX" => "Aeromexico",
        "CMP" => "Copa Airlines",
        "SWA" => "Southwest",
        "JBU" => "JetBlue",
        "ASA" => "Alaska Airlines",
        "FDX" => "FedEx",
        "UPS" => "UPS",
        other => return other.to_string(),
    };
    name.to_string()
}

#[derive(Clone, Debug, Default)]
pub struct Briefing {
    pub callsign: String,
    pub airline: String,
    pub airline_code: String,
    pub flight_number: String,
    pub aircraft_name: String,
    pub aircraft_type: String,
    pub registration: String,
    pub cruise_ft: f64,
    pub route_text: String,
    pub route_distance_nm: f64,
    pub planned_ete_s: f64,
    pub block_fuel_kg: f64,
    pub passengers: i64,
    pub origin_name: String,
    pub dest_name: String,
    pub origin_metar: String,
    pub dest_metar: String,
    pub units: String,
    pub photo_query: String,
    pub fetched_at: u64,
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn text_at(value: &Value, key: &str) -> String {
    value.get(key).map(text).unwrap_or_default()
}

fn number_at(value: &Value, key: &str) -> Option<f64> {
    let raw = value.get(key)?;
    match raw {
        Value::String(s) => s.trim().replace(',', ".").parse::<f64>().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Downloads and parses the pilot's latest SimBrief OFP.
pub fn fetch(user: &str) -> Result<(FlightPlan, Briefing), String> {
    let user = user.trim();
    if user.is_empty() {
        return Err("Insert your SimBrief username or pilot ID first".to_string());
    }
    let param = if user.chars().all(|c| c.is_ascii_digit()) {
        "userid"
    } else {
        "username"
    };
    let url = format!(
        "https://www.simbrief.com/api/xml.fetcher.php?{}={}&json=1",
        param,
        net::encode_component(user)
    );
    let json = net::get_json(&url)?;

    let status = text_at(&json["fetch"], "status");
    if status != "Success" {
        return Err(if status.is_empty() {
            "SimBrief did not return a flight plan for this user".to_string()
        } else {
            format!("SimBrief: {status}")
        });
    }

    let general = &json["general"];
    let origin = &json["origin"];
    let destination = &json["destination"];
    let aircraft = &json["aircraft"];
    let fuel = &json["fuel"];
    let times = &json["times"];
    let atc = &json["atc"];
    let units = text_at(&json["params"], "units");

    let origin_icao = text_at(origin, "icao_code");
    let dest_icao = text_at(destination, "icao_code");
    if origin_icao.is_empty() || dest_icao.is_empty() {
        return Err("SimBrief plan has no route".to_string());
    }

    let origin_pos = match (
        number_at(origin, "pos_lat"),
        number_at(origin, "pos_long"),
    ) {
        (Some(lat), Some(lon)) => Some((lat, lon)),
        _ => None,
    };
    let dest_pos = match (
        number_at(destination, "pos_lat"),
        number_at(destination, "pos_long"),
    ) {
        (Some(lat), Some(lon)) => Some((lat, lon)),
        _ => None,
    };

    let fixes: Vec<Value> = match &json["navlog"]["fix"] {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![json["navlog"]["fix"].clone()],
        _ => Vec::new(),
    };

    let mut route: Vec<Waypoint> = Vec::new();
    for fix in &fixes {
        let (Some(lat), Some(lon)) = (
            number_at(fix, "pos_lat"),
            number_at(fix, "pos_long"),
        ) else {
            continue;
        };
        if lat.abs() > 90.0 || lon.abs() > 180.0 {
            continue;
        }
        let ident = {
            let id = text_at(fix, "ident");
            if id.is_empty() {
                text_at(fix, "name")
            } else {
                id
            }
        };
        let kind = text_at(fix, "type");
        route.push(Waypoint {
            id: if ident.is_empty() { "FIX".to_string() } else { ident },
            lat,
            lon,
            kind: match kind.as_str() {
                "apt" => "Airport".to_string(),
                "vor" => "VOR".to_string(),
                "ndb" => "NDB".to_string(),
                "ltlg" => "Lat/Lon".to_string(),
                _ => "Waypoint".to_string(),
            },
        });
    }

    if route.len() < 2 {
        return Err("SimBrief plan contains no navigable fixes".to_string());
    }

    let cruise_ft = number_at(general, "initial_altitude").unwrap_or(f64::NAN);
    let fuel_raw = number_at(fuel, "plan_ramp").unwrap_or(0.0);
    let block_fuel_kg = if units.eq_ignore_ascii_case("lbs") {
        fuel_raw * LB_TO_KG
    } else {
        fuel_raw
    };

    let registration = text_at(aircraft, "reg");
    let aircraft_type = {
        let t = text_at(aircraft, "icaocode");
        if t.is_empty() {
            text_at(aircraft, "base_type")
        } else {
            t
        }
    };
    let aircraft_name = text_at(aircraft, "name");
    let airline_code = text_at(general, "icao_airline");
    let airline = airline_name(&airline_code);
    let flight_number = text_at(general, "flight_number");
    let mut callsign = text_at(atc, "callsign");
    if callsign.is_empty() {
        callsign = format!("{airline_code}{flight_number}");
    }
    if callsign.trim().is_empty() {
        callsign = registration.clone();
    }

    let photo_query = if aircraft_name.is_empty() {
        aircraft_type.clone()
    } else {
        aircraft_name.clone()
    };

    let briefing = Briefing {
        callsign: callsign.clone(),
        airline,
        airline_code,
        flight_number,
        aircraft_name: aircraft_name.clone(),
        aircraft_type,
        registration,
        cruise_ft,
        route_text: text_at(general, "route"),
        route_distance_nm: number_at(general, "route_distance").unwrap_or(0.0),
        planned_ete_s: number_at(times, "est_time_enroute").unwrap_or(f64::NAN),
        block_fuel_kg,
        passengers: number_at(general, "passengers").map(|v| v as i64).unwrap_or(0),
        origin_name: text_at(origin, "name"),
        dest_name: text_at(destination, "name"),
        origin_metar: text_at(origin, "metar"),
        dest_metar: text_at(destination, "metar"),
        units,
        photo_query,
        fetched_at: now_epoch(),
    };

    let plan = FlightPlan {
        title: format!("{origin_icao} to {dest_icao}"),
        origin: origin_icao,
        dest: dest_icao,
        origin_pos,
        dest_pos,
        dest_elev_ft: number_at(destination, "elevation").unwrap_or(0.0),
        cruise_ft,
        route,
        path: None,
        source: if callsign.is_empty() {
            "SimBrief".to_string()
        } else {
            format!("SimBrief · {callsign}")
        },
    };

    Ok((plan, briefing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_airline_from_the_icao_code() {
        assert_eq!(airline_name("DLH"), "Lufthansa");
        assert_eq!(airline_name("dlh"), "Lufthansa");
        assert_eq!(airline_name("UAE"), "Emirates");
        assert_eq!(airline_name("ZZZ"), "ZZZ");
        assert_eq!(airline_name(""), "");
    }
}
