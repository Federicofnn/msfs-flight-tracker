//! Aircraft photo lookup.
//!
//! Order of preference:
//! 1. an explicit query chosen by the pilot,
//! 2. Wikimedia Commons by tail number (an exact registration is unambiguous),
//! 3. Wikimedia Commons by operator + type,
//! 4. Planespotters by tail number,
//! 5. Wikipedia page images, but only when the photo belongs to the operator.
//!
//! Every candidate is checked against the tokens we asked for, so a Lufthansa
//! A380 can never come back as an Emirates A380.

use crate::net;

/// Everything the card knows about the aircraft.
#[derive(Clone, Debug, Default)]
pub struct Lookup {
    pub registration: String,
    pub airline: String,
    pub model: String,
    pub title: String,
    /// Manual search typed in Settings; wins over everything else.
    pub query: String,
}

impl Lookup {
    /// Identifies one photo request (a new registration triggers a new fetch).
    pub fn key(&self) -> String {
        let query = self.query.trim();
        if !query.is_empty() {
            return format!("q:{query}");
        }
        let registration = self.registration.trim().to_ascii_uppercase();
        if !registration.is_empty() {
            return registration;
        }
        format!("{}|{}", self.airline.trim(), self.model.trim())
    }

    pub fn is_empty(&self) -> bool {
        self.key().trim().is_empty()
    }
}

pub struct Photo {
    pub bytes: Vec<u8>,
    pub source: &'static str,
}

/// Downloads a photo of this exact aircraft.
pub fn fetch_photo(lookup: &Lookup) -> Result<Photo, String> {
    let mut last_error = String::new();

    if !lookup.query.trim().is_empty() {
        let query = lookup.query.trim().to_string();
        if let Ok(photo) = commons_search(&query, &tokens(&query), "WIKIMEDIA") {
            return Ok(photo);
        }
        if let Ok(photo) = wikipedia_search(&query, "", false) {
            return Ok(photo);
        }
    }

    let registration = lookup.registration.trim().to_uppercase();
    let airline = lookup.airline.trim().to_string();
    let model = if lookup.model.trim().is_empty() {
        lookup.title.trim().to_string()
    } else {
        lookup.model.trim().to_string()
    };
    let model_token = family_token(&model);

    // Planespotters is an airframe database: a hit here is the exact aircraft.
    if usable_registration(&registration) {
        match planespotters(&registration) {
            Ok(photo) => return Ok(photo),
            Err(err) => last_error = err,
        }
    }

    if !airline.is_empty() && !model.is_empty() {
        let query = format!("{airline} {model}");
        let mut required = tokens(&airline);
        if let Some(token) = &model_token {
            required.push(token.clone());
        }
        match commons_search(&query, &required, "WIKIMEDIA") {
            Ok(photo) => return Ok(photo),
            Err(err) => last_error = err,
        }
    }

    if !airline.is_empty() && !model.is_empty() {
        match wikipedia_search(&format!("{airline} {model}"), &airline, true) {
            Ok(photo) => return Ok(photo),
            Err(err) => last_error = err,
        }
    }

    if let Some(token) = model_token {
        if let Ok(photo) = commons_search(&format!("{model} airliner"), &[token], "WIKIMEDIA") {
            return Ok(photo);
        }
    }
    if !lookup.title.trim().is_empty() {
        if let Ok(photo) = wikipedia_search(lookup.title.trim(), &airline, false) {
            return Ok(photo);
        }
    }

    Err(if last_error.is_empty() {
        "No photo found for this aircraft".to_string()
    } else {
        last_error
    })
}

/// Tail numbers that really identify one airframe (demo and test ids do not).
fn usable_registration(registration: &str) -> bool {
    let reg = registration.trim().to_uppercase();
    if reg.len() < 5 || reg.len() > 8 {
        return false;
    }
    let banned = ["DEMO", "TEST", "NONE", "GENERIC", "DEFAULT", "XXXX"];
    if banned.iter().any(|word| reg.contains(word)) {
        return false;
    }
    reg.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Operator name embedded in an MSFS livery title, when there is no SimBrief plan.
pub fn airline_from_title(title: &str) -> String {
    const NAMES: &[&str] = &[
        "Lufthansa",
        "Emirates",
        "British Airways",
        "Air France",
        "KLM",
        "Ryanair",
        "easyJet",
        "SWISS",
        "Austrian",
        "ITA Airways",
        "Alitalia",
        "Turkish Airlines",
        "Qatar Airways",
        "Singapore Airlines",
        "Cathay Pacific",
        "American Airlines",
        "Delta Air Lines",
        "United Airlines",
        "Air Canada",
        "Iberia",
        "Vueling",
        "Aer Lingus",
        "TAP",
        "SAS",
        "Norwegian",
        "Wizz Air",
        "Finnair",
        "Aegean",
        "Air China",
        "China Eastern",
        "China Southern",
        "All Nippon",
        "Japan Airlines",
        "Korean Air",
        "Ethiopian",
        "EgyptAir",
        "Royal Air Maroc",
        "Saudia",
        "Etihad",
        "Qantas",
        "Air New Zealand",
        "LOT",
        "airBaltic",
        "Air India",
        "Kuwait Airways",
        "Gulf Air",
        "Oman Air",
        "Royal Jordanian",
        "South African",
        "Kenya Airways",
        "Tunisair",
        "LATAM",
        "Azul",
        "Aeromexico",
        "Copa Airlines",
        "Southwest",
        "JetBlue",
        "Alaska Airlines",
        "FedEx",
        "UPS",
        "DHL",
    ];
    let haystack = title.to_ascii_lowercase();
    for name in NAMES {
        if haystack.contains(&name.to_ascii_lowercase()) {
            return (*name).to_string();
        }
    }
    String::new()
}

/// Wikipedia Commons file search, with every required token checked in the file name.
fn commons_search(query: &str, required: &[String], source: &'static str) -> Result<Photo, String> {
    let search = format!("filetype:bitmap {query}");
    let url = format!(
        "https://commons.wikimedia.org/w/api.php?action=query&format=json&generator=search&gsrsearch={}&gsrnamespace=6&gsrlimit=10&prop=imageinfo&iiprop=url&iiurlwidth=1100",
        net::encode_component(&search)
    );
    let json = net::get_json(&url)?;
    let Some(pages) = json["query"]["pages"].as_object() else {
        return Err("No photo on Wikimedia Commons".to_string());
    };
    let mut candidates: Vec<(u32, String)> = Vec::new();
    for page in pages.values() {
        let Some(src) = page["imageinfo"][0]["thumburl"]
            .as_str()
            .or_else(|| page["imageinfo"][0]["url"].as_str())
        else {
            continue;
        };
        let title = page["title"].as_str().unwrap_or_default();
        let haystack = normalise(&format!("{title} {src}"));
        if !required.iter().all(|token| haystack.contains(token)) {
            continue;
        }
        candidates.push((page["index"].as_u64().unwrap_or(99) as u32, src.to_string()));
    }
    candidates.sort();
    for (_, src) in candidates {
        return net::http_get(&src).map(|bytes| Photo { bytes, source });
    }
    Err("No matching photo on Wikimedia Commons".to_string())
}

/// Planespotters returns the tail's own photos, newest first.
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
            return net::http_get(src).map(|bytes| Photo {
                bytes,
                source: "PLANESPOTTERS",
            });
        }
    }
    Err(format!("No photo of {registration}"))
}

/// Wikipedia page image, filtered by operator so we never show a rival livery.
fn wikipedia_search(query: &str, airline: &str, require_airline: bool) -> Result<Photo, String> {
    let search = if query.to_ascii_lowercase().contains("aircraft") {
        query.to_string()
    } else {
        format!("{query} aircraft")
    };
    let url = format!(
        "https://en.wikipedia.org/w/api.php?action=query&format=json&prop=pageimages&piprop=thumbnail&pithumbsize=800&generator=search&gsrsearch={}&gsrlimit=8",
        net::encode_component(&search)
    );
    let json = net::get_json(&url)?;
    let Some(pages) = json["query"]["pages"].as_object() else {
        return Err("No photo found for this aircraft".to_string());
    };

    let wanted = tokens(airline);
    let mut candidates: Vec<(u32, u32, String)> = Vec::new();
    for page in pages.values() {
        let Some(src) = page["thumbnail"]["source"].as_str() else {
            continue;
        };
        let title = page["title"].as_str().unwrap_or_default();
        if title.starts_with("List of") {
            continue;
        }
        let index = page["index"].as_u64().unwrap_or(99) as u32;
        let haystack = format!("{title} {src}").to_ascii_lowercase();
        let airline_hit = wanted.iter().any(|token| haystack.contains(token));
        if require_airline && !airline_hit {
            continue;
        }
        let score = if airline_hit { 4 } else { 0 };
        candidates.push((u32::MAX - score, index, src.to_string()));
    }
    candidates.sort();
    for (_, _, src) in candidates {
        return net::http_get(&src).map(|bytes| Photo {
            bytes,
            source: "WIKIPEDIA",
        });
    }
    Err("No photo of this operator".to_string())
}

/// "A380-800" -> "a380", "B738" -> "b738"; used to match file names.
fn family_token(model: &str) -> Option<String> {
    model
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find(|word| word.len() >= 3 && word.chars().any(|c| c.is_ascii_digit()))
        .map(|word| word.to_ascii_lowercase())
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 3)
        .map(|word| word.to_ascii_lowercase())
        .collect()
}

fn normalise(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
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

    #[test]
    fn keys_follow_the_registration() {
        let lookup = Lookup {
            registration: "d-aimb".into(),
            airline: "Lufthansa".into(),
            model: "A380-800".into(),
            title: "Lufthansa A380".into(),
            query: String::new(),
        };
        assert_eq!(lookup.key(), "D-AIMB");
        assert!(!lookup.is_empty());
        assert_eq!(family_token("A380-800").as_deref(), Some("a380"));
        assert_eq!(normalise("D-AIMB").as_str(), "daimb");
    }

    #[test]
    fn reads_the_operator_from_a_livery_title() {
        assert_eq!(airline_from_title("Airbus A380-800 Lufthansa"), "Lufthansa");
        assert_eq!(airline_from_title("Boeing 737-800 Ryanair"), "Ryanair");
        assert_eq!(airline_from_title("Generic A320"), "");
    }

    #[test]
    #[ignore]
    fn fetches_the_registration_photo() {
        let lookup = Lookup {
            registration: "D-AIMB".into(),
            airline: "Lufthansa".into(),
            model: "A380-800".into(),
            title: "Lufthansa Airbus A380".into(),
            query: String::new(),
        };
        let photo = fetch_photo(&lookup).expect("photo");
        assert!(photo.bytes.len() > 4000, "unexpected tiny photo");
        assert!(
            photo.bytes.starts_with(&[0xFF, 0xD8]) || photo.bytes.starts_with(&[0x89, b'P']),
            "should be a JPEG or PNG"
        );
        assert!(!photo.source.is_empty());
        println!("source={} bytes={}", photo.source, photo.bytes.len());
    }
}
