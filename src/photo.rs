//! Aircraft photo lookup driven by the callsign.
//!
//! The callsign (or, when there is no SimBrief plan, the livery title) tells us
//! the operator; the model tells us the family. A photo is accepted only when
//! its file name names that operator and that family, so a random airliner can
//! never appear. A tail number is used only when it really looks like one, and
//! then Planespotters' airframe database answers for the exact aircraft.

use crate::net;
use crate::simbrief;

/// Every operator the app can recognise in a livery title or callsign.
const KNOWN_AIRLINES: &[&str] = &[
    "Aegean Airlines",
    "Aer Lingus",
    "Aerolineas Argentinas",
    "Aeromexico",
    "Air Canada",
    "Air China",
    "Air France",
    "Air India",
    "Air New Zealand",
    "airBaltic",
    "Alaska Airlines",
    "Alitalia",
    "All Nippon Airways",
    "American Airlines",
    "Austrian Airlines",
    "Azul",
    "British Airways",
    "Cathay Pacific",
    "China Eastern",
    "China Southern",
    "Copa Airlines",
    "Delta Air Lines",
    "easyJet",
    "EgyptAir",
    "Emirates",
    "Ethiopian Airlines",
    "Etihad Airways",
    "FedEx",
    "Finnair",
    "Gulf Air",
    "Iberia",
    "IndiGo",
    "ITA Airways",
    "Japan Airlines",
    "JetBlue",
    "Kenya Airways",
    "KLM",
    "Korean Air",
    "Kuwait Airways",
    "LATAM",
    "LOT Polish Airlines",
    "Lufthansa",
    "Middle East Airlines",
    "Norwegian",
    "Oman Air",
    "Qantas",
    "Qatar Airways",
    "Royal Air Maroc",
    "Royal Jordanian",
    "Ryanair",
    "SAS",
    "Saudia",
    "Singapore Airlines",
    "South African Airways",
    "Southwest",
    "SpiceJet",
    "SWISS",
    "TAP Air Portugal",
    "Tunisair",
    "Turkish Airlines",
    "United Airlines",
    "UPS",
    "Vueling",
    "Wizz Air",
];

/// Everything the card knows about the aircraft.
#[derive(Clone, Debug, Default)]
pub struct Lookup {
    pub registration: String,
    pub callsign: String,
    pub airline: String,
    pub model: String,
    pub title: String,
    /// Manual search typed in Settings; wins over everything else.
    pub query: String,
}

impl Lookup {
    /// Identifies one photo request (a new registration or operator triggers a
    /// new fetch).
    pub fn key(&self) -> String {
        let query = self.query.trim();
        if !query.is_empty() {
            return format!("q:{query}");
        }
        let registration = self.registration.trim().to_ascii_uppercase();
        if usable_registration(&registration) {
            return registration;
        }
        let airline = self.airline_name();
        if !airline.is_empty() {
            return format!("a:{}|{}", airline, self.model.trim());
        }
        let callsign = self.callsign.trim().to_ascii_uppercase();
        if !callsign.is_empty() {
            return format!("c:{callsign}");
        }
        format!("t:{}", self.title.trim())
    }

    pub fn is_empty(&self) -> bool {
        self.key().trim().is_empty()
    }

    /// SimBrief's operator, the livery title, or the callsign prefix, in that order.
    pub fn airline_name(&self) -> String {
        let briefing = self.airline.trim();
        if !briefing.is_empty() {
            return briefing.to_string();
        }
        let livery = airline_from_title(&self.title);
        if !livery.is_empty() {
            return livery;
        }
        let from_callsign = airline_from_callsign(&self.callsign);
        if !from_callsign.is_empty() {
            return from_callsign;
        }
        airline_from_title(&self.callsign)
    }
}

pub struct Photo {
    pub bytes: Vec<u8>,
    pub source: &'static str,
    /// What the photo claims to show, e.g. "ITA Airways · A320".
    pub label: String,
    /// Attribution line, e.g. "© Jane Doe · CC BY-SA 4.0 · Wikimedia Commons".
    pub credit: String,
}

/// Downloads a photo of this exact aircraft.
pub fn fetch_photo(lookup: &Lookup) -> Result<Photo, String> {
    let manual = lookup.query.trim();
    if !manual.is_empty() {
        return commons_query(manual, None, &[]).map_err(|_| {
            format!("No photo matches \"{manual}\". Try the registration, e.g. EI-INA.")
        });
    }

    let airline = lookup.airline_name();
    let families = families(&format!("{} {}", lookup.model, lookup.title));
    let registration = lookup.registration.trim().to_uppercase();
    let mut problem = String::new();

    if usable_registration(&registration) && Some(&registration) != Some(&lookup.callsign.trim().to_uppercase()) {
        match planespotters(&registration) {
            Ok(photo) => return Ok(photo),
            Err(err) => problem = err,
        }
    }

    if !airline.is_empty() && !families.is_empty() {
        match commons_aircraft(&airline, &families) {
            Ok(photo) => return Ok(photo),
            Err(err) => problem = err,
        }
    } else if !airline.is_empty() {
        if let Ok(photo) = commons_query(&airline, Some(&airline), &[]) {
            return Ok(photo);
        }
    }

    Err(if airline.is_empty() {
        format!(
            "Photo not assigned: no operator known{}. Load a SimBrief plan or type a search in Settings.",
            if lookup.callsign.trim().is_empty() {
                String::new()
            } else {
                format!(" for {}", lookup.callsign.trim())
            }
        )
    } else if families.is_empty() {
        format!("Photo not assigned: {airline} known, aircraft type unknown")
    } else if problem.is_empty() {
        format!("No {airline} photo of this aircraft type found")
    } else {
        format!("{problem} · {airline}")
    })
}

/// Operator name embedded in an MSFS livery title.
pub fn airline_from_title(title: &str) -> String {
    for name in KNOWN_AIRLINES {
        if contains_name(title, name) {
            return (*name).to_string();
        }
    }
    String::new()
}

/// ICAO prefix of the callsign ("ITY610" -> "ITA Airways").
pub fn airline_from_callsign(callsign: &str) -> String {
    // SimBrief gives "ITY610"; the simulator gives "ITA Airways" + "610".
    for word in callsign.split_whitespace() {
        let prefix: String = word
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_uppercase();
        if prefix.len() < 2 || prefix.len() > 3 {
            continue;
        }
        let name = simbrief::airline_name(&prefix);
        if !name.eq_ignore_ascii_case(&prefix) {
            return name;
        }
    }
    String::new()
}

/// A tail number that identifies one airframe (callsigns and demo ids do not).
fn usable_registration(registration: &str) -> bool {
    let reg = registration.trim().to_ascii_uppercase();
    if reg.len() < 4 || reg.len() > 8 {
        return false;
    }
    let banned = ["DEMO", "TEST", "NONE", "GENERIC", "DEFAULT", "XXXX"];
    if banned.iter().any(|word| reg.contains(word)) {
        return false;
    }
    let dashed = match reg.split_once('-') {
        Some((left, right)) => {
            (1..=2).contains(&left.len())
                && (2..=5).contains(&right.len())
                && reg.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        }
        None => false,
    };
    let american = reg.starts_with('N')
        && reg.len() >= 4
        && reg[1..].chars().all(|c| c.is_ascii_alphanumeric())
        && reg[1..].chars().any(|c| c.is_ascii_digit());
    dashed || american
}

/// Aircraft family tokens as they appear in file names ("A20N" -> "a320").
fn families(text: &str) -> Vec<String> {
    let code = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|word| word.to_ascii_uppercase())
        .find(|word| {
            (word.len() == 3 || word.len() == 4)
                && word.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && word.chars().filter(|c| c.is_ascii_digit()).count() >= 2
        })
        .unwrap_or_default();
    let mapped: &[&str] = match code.as_str() {
        "A319" => &["a319"],
        "A320" | "A20N" => &["a320"],
        "A321" | "A21N" => &["a321"],
        "A332" | "A333" | "A339" | "A337" => &["a330"],
        "A343" | "A345" | "A346" => &["a340"],
        "A359" | "A35K" => &["a350"],
        "A388" | "A380" => &["a380"],
        "B737" | "B738" | "B739" | "B38M" | "B39M" | "B736" => &["737"],
        "B742" | "B743" | "B744" | "B748" => &["747"],
        "B752" | "B753" => &["757"],
        "B762" | "B763" | "B764" => &["767"],
        "B772" | "B773" | "B77W" | "B77L" => &["777"],
        "B788" | "B789" | "B78X" => &["787"],
        "E170" | "E175" => &["e170", "e175"],
        "E190" | "E195" | "E290" | "E295" => &["e190", "e195"],
        "CRJ7" | "CRJ9" | "CRJX" => &["crj"],
        "AT72" | "AT75" | "AT76" => &["atr"],
        "DH8D" => &["dash 8", "q400"],
        "MD11" => &["md-11", "md11"],
        _ => &[],
    };
    if !mapped.is_empty() {
        return mapped.iter().map(|token| (*token).to_string()).collect();
    }

    // Fall back to what the title says: "Airbus A320neo", "Boeing 737-800"…
    let squashed = squashed(text);
    let mut found = Vec::new();
    for (needle, token) in [
        ("a319", "a319"),
        ("a320", "a320"),
        ("a321", "a321"),
        ("a330", "a330"),
        ("a340", "a340"),
        ("a350", "a350"),
        ("a380", "a380"),
        ("737", "737"),
        ("747", "747"),
        ("757", "757"),
        ("767", "767"),
        ("777", "777"),
        ("787", "787"),
        ("e190", "e190"),
        ("e195", "e195"),
        ("crj", "crj"),
        ("atr", "atr"),
    ] {
        if squashed.contains(needle) {
            found.push(token.to_string());
        }
    }
    found
}

fn commons_aircraft(airline: &str, families: &[String]) -> Result<Photo, String> {
    let family_text = families.join(" ");
    let query = format!("{airline} {family_text}");
    let label = format!("{airline} · {}", families[0].to_uppercase());
    commons_query(&query, Some(airline), families).map(|photo| Photo { label, ..photo })
}

/// Commons file search restricted to one operator and one aircraft family.
fn commons_query(
    query: &str,
    airline: Option<&str>,
    families: &[String],
) -> Result<Photo, String> {
    let search = format!("filetype:bitmap {query}");
    let url = format!(
        "https://commons.wikimedia.org/w/api.php?action=query&format=json&generator=search&gsrsearch={}&gsrnamespace=6&gsrlimit=25&prop=imageinfo&iiprop=url|extmetadata&iiurlwidth=1100",
        net::encode_component(&search)
    );
    let json = net::get_json(&url)?;
    let Some(pages) = json["query"]["pages"].as_object() else {
        return Err("No photo on Wikimedia Commons".to_string());
    };
    let wanted_families = if families.is_empty() {
        Vec::new()
    } else {
        families.to_vec()
    };

    let mut candidates: Vec<(u32, String, String)> = Vec::new();
    for page in pages.values() {
        let Some(src) = page["imageinfo"][0]["thumburl"]
            .as_str()
            .or_else(|| page["imageinfo"][0]["url"].as_str())
        else {
            continue;
        };
        let title = page["title"].as_str().unwrap_or_default();
        if let Some(airline) = airline {
            if !airline_matches(title, airline) {
                continue;
            }
            if shows_another_airline(title, airline) {
                continue;
            }
        }
        if !wanted_families.is_empty() {
            let file_key = squashed(title);
            if !wanted_families
                .iter()
                .any(|family| file_key.contains(&squashed(family)))
            {
                continue;
            }
        }
        candidates.push((
            page["index"].as_u64().unwrap_or(99) as u32,
            src.to_string(),
            commons_credit(&page["imageinfo"][0]["extmetadata"]),
        ));
    }
    candidates.sort();
    for (_, src, credit) in candidates {
        if let Ok(bytes) = net::http_get(&src) {
            return Ok(Photo {
                bytes,
                source: "WIKIMEDIA",
                label: query.to_string(),
                credit,
            });
        }
    }
    Err(format!("No matching photo for {query}"))
}

/// Planespotters answers by tail number: the exact airframe.
fn planespotters(registration: &str) -> Result<Photo, String> {
    let url = format!(
        "https://api.planespotters.net/pub/photos/reg/{}",
        net::encode_component(registration)
    );
    let json = net::get_json(&url)?;
    let photos = json["photos"]
        .as_array()
        .ok_or_else(|| format!("No photo of {registration}"))?;
    for photo in photos {
        let src = photo["thumbnail_large"]["src"]
            .as_str()
            .or_else(|| photo["thumbnail"]["src"].as_str());
        if let Some(src) = src {
            let photographer = photo["photographer"].as_str().unwrap_or_default().trim();
            let credit = if photographer.is_empty() {
                "© Planespotters.net".to_string()
            } else {
                format!("© {photographer} · Planespotters.net")
            };
            return net::http_get(src).map(|bytes| Photo {
                bytes,
                source: "PLANESPOTTERS",
                label: registration.to_string(),
                credit,
            });
        }
    }
    Err(format!("No photo of {registration}"))
}

/// Pulls the author and licence out of a Commons file, so the photo is
/// attributed instead of copied without credit.
fn commons_credit(metadata: &serde_json::Value) -> String {
    let artist = strip_html(metadata["Artist"]["value"].as_str().unwrap_or_default());
    let licence = strip_html(metadata["LicenseShortName"]["value"].as_str().unwrap_or_default());
    let mut parts = Vec::new();
    if !artist.is_empty() {
        parts.push(format!("© {artist}"));
    }
    if !licence.is_empty() {
        parts.push(licence);
    }
    parts.push("Wikimedia Commons".to_string());
    parts.join(" · ")
}

fn strip_html(value: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in value.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Word-sequence match so "ITA" never matches inside "Capital".
fn contains_name(title: &str, name: &str) -> bool {
    let title_words = words(title);
    let target = words(name);
    if target.is_empty() {
        return false;
    }
    title_words
        .windows(target.len())
        .any(|window| window == target)
}

fn airline_matches(title: &str, airline: &str) -> bool {
    if contains_name(title, airline) {
        return true;
    }
    airline_aliases(airline)
        .iter()
        .any(|alias| contains_name(title, alias))
}

/// Short names the file names use for operators with long official names.
fn airline_aliases(airline: &str) -> Vec<String> {
    let aliases: &[&str] = match airline {
        "ITA Airways" => &["ITA"],
        "British Airways" => &["British Airways", "British"],
        "TAP Air Portugal" => &["TAP"],
        "LOT Polish Airlines" => &["LOT"],
        "All Nippon Airways" => &["ANA"],
        "SAS" => &["SAS", "Scandinavian"],
        "SWISS" => &["Swiss"],
        "American Airlines" => &["American"],
        "Delta Air Lines" => &["Delta"],
        "United Airlines" => &["United"],
        "Turkish Airlines" => &["Turkish"],
        "Qatar Airways" => &["Qatar"],
        "Singapore Airlines" => &["Singapore"],
        "Ethiopian Airlines" => &["Ethiopian"],
        "EgyptAir" => &["EgyptAir"],
        "Royal Air Maroc" => &["Royal Air Maroc", "RAM"],
        "Air New Zealand" => &["Air New Zealand", "Air NZ"],
        "Austrian Airlines" => &["Austrian"],
        "Aegean Airlines" => &["Aegean"],
        "Alaska Airlines" => &["Alaska"],
        "China Eastern" => &["China Eastern"],
        "China Southern" => &["China Southern"],
        "Air Canada" => &["Air Canada"],
        "Air China" => &["Air China"],
        _ => &[],
    };
    aliases.iter().map(|alias| (*alias).to_string()).collect()
}

/// Rejects a file that names a different operator ("Alitalia" for an ITA search).
fn shows_another_airline(title: &str, airline: &str) -> bool {
    KNOWN_AIRLINES.iter().any(|other| {
        !other.eq_ignore_ascii_case(airline)
            && contains_name(title, other)
    })
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word.to_ascii_lowercase())
        .collect()
}

fn squashed(text: &str) -> String {
    words(text).join("")
}


/// Decodes JPEG/PNG bytes into something egui can upload as a texture.
pub fn decode(bytes: &[u8]) -> Result<eframe::egui::ColorImage, String> {
    let image = image::load_from_memory(bytes).map_err(|e| format!("Unsupported image: {e}"))?;
    let image = image.thumbnail(760, 480);
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(eframe::egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(callsign: &str, title: &str, model: &str) -> Lookup {
        Lookup {
            registration: String::new(),
            callsign: callsign.into(),
            airline: String::new(),
            model: model.into(),
            title: title.into(),
            query: String::new(),
        }
    }

    #[test]
    fn reads_the_operator_from_the_callsign() {
        assert_eq!(airline_from_callsign("ITY610"), "ITA Airways");
        assert_eq!(airline_from_callsign("DLH4AB"), "Lufthansa");
        assert_eq!(airline_from_callsign("SWR123"), "SWISS");
        assert_eq!(airline_from_callsign("ZZZ1"), "");
        assert_eq!(airline_from_callsign("ITA Airways 610"), "ITA Airways");
    }

    #[test]
    fn reads_the_operator_from_a_livery_title() {
        assert_eq!(airline_from_title("Airbus A380-800 Lufthansa"), "Lufthansa");
        assert_eq!(airline_from_title("Boeing 737-800 Ryanair"), "Ryanair");
        assert_eq!(airline_from_title("A320 ITA Airways"), "ITA Airways");
        assert_eq!(airline_from_title("Generic A320"), "");
        assert_eq!(airline_from_title("Capital Airlines A320"), "");
    }

    #[test]
    fn matches_file_names_by_operator_and_family() {
        assert!(airline_matches("ITA Airways Airbus A320-216", "ITA Airways"));
        assert!(airline_matches("EI-INA ITA A320", "ITA Airways"));
        assert!(airline_matches("EI-INA ITA Airways A20N", "ITA Airways"));
        assert!(!airline_matches("Capital Airlines A320", "ITA Airways"));
        assert!(shows_another_airline("Alitalia A320", "ITA Airways"));
        assert!(!shows_another_airline("ITA Airways A320", "ITA Airways"));
        assert_eq!(families("A20N").first().map(String::as_str), Some("a320"));
        assert_eq!(families("Boeing 737-800").first().map(String::as_str), Some("737"));
        assert_eq!(families("Airbus A380-800 Lufthansa").first().map(String::as_str), Some("a380"));
    }

    #[test]
    fn keys_follow_the_operator() {
        let mut plane = lookup("ITY610", "A320 ITA Airways", "A320");
        assert_eq!(plane.key(), "a:ITA Airways|A320");
        assert_eq!(plane.airline_name(), "ITA Airways");
        plane.registration = "I-DEMO".into();
        assert_eq!(plane.key(), "a:ITA Airways|A320");
        plane.registration = "EI-INA".into();
        assert_eq!(plane.key(), "EI-INA");
        assert!(!usable_registration("ITY610"));
        assert!(usable_registration("EI-INA"));
        assert!(usable_registration("N12345"));
    }

    #[test]
    #[ignore]
    fn fetches_the_registration_photo() {
        let plane = Lookup {
            registration: "D-AIMB".into(),
            callsign: "DLH400".into(),
            airline: "Lufthansa".into(),
            model: "A380-800".into(),
            title: "Lufthansa Airbus A380".into(),
            query: String::new(),
        };
        let photo = fetch_photo(&plane).expect("photo");
        assert!(photo.bytes.len() > 4000, "unexpected tiny photo");
        println!("source={} label={} bytes={}", photo.source, photo.label, photo.bytes.len());
    }

    #[test]
    #[ignore]
    fn fetches_an_operator_photo_from_the_callsign() {
        let plane = lookup("ITY610", "A320 ITA Airways", "A320");
        let photo = fetch_photo(&plane).expect("photo");
        assert!(photo.bytes.len() > 4000);
        println!("source={} label={} bytes={}", photo.source, photo.label, photo.bytes.len());
    }
}
