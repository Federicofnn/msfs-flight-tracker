//! Persistence: flight records on disk, settings, stats and exports.

use crate::flight::FlightRecord;
use crate::util::{fmt_utc_full, now_epoch};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub imperial: bool,
    pub auto_record: bool,
    pub sample_secs: f64,
    pub last_plan: Option<String>,
    pub show_in_hg: bool,
    pub simbrief_id: String,
    pub photo_query: String,
    pub check_updates: bool,
    pub update_url: String,
    /// GitHub token with the gist permission, used only for the logbook sync.
    pub github_token: String,
    /// The logbook gist; empty until the first upload.
    pub github_gist: String,
    pub sync_auto: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            imperial: false,
            auto_record: true,
            sample_secs: 2.0,
            last_plan: None,
            show_in_hg: false,
            simbrief_id: String::new(),
            photo_query: String::new(),
            check_updates: true,
            update_url: String::new(),
            github_token: String::new(),
            github_gist: String::new(),
            sync_auto: false,
        }
    }
}

#[derive(Default, Clone)]
pub struct Stats {
    pub flights: usize,
    pub total_hours: f64,
    pub total_nm: f64,
    pub avg_landing_fpm: f64,
    pub best_landing_fpm: f64,
    pub worst_landing_fpm: f64,
    pub landings: usize,
    pub avg_duration_s: f64,
    pub top_aircraft: String,
    pub top_aircraft_count: usize,
}

pub struct Store {
    pub dir: PathBuf,
    pub flights: Vec<FlightRecord>,
    pub settings: Settings,
    pub load_errors: usize,
}

impl Store {
    pub fn open(demo: bool) -> Self {
        let mut dir = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("MSFSFlightTracker");
        if demo {
            dir = dir.join("demo");
        }
        let _ = std::fs::create_dir_all(dir.join("flights"));
        let _ = std::fs::create_dir_all(dir.join("exports"));

        let settings = std::fs::read_to_string(dir.join("settings.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default();

        let mut store = Store {
            dir,
            flights: Vec::new(),
            settings,
            load_errors: 0,
        };
        store.reload();
        store
    }

    pub fn flights_dir(&self) -> PathBuf {
        self.dir.join("flights")
    }

    pub fn exports_dir(&self) -> PathBuf {
        self.dir.join("exports")
    }

    pub fn save_settings(&self) {
        let path = self.dir.join("settings.json");
        if let Ok(text) = serde_json::to_string_pretty(&self.settings) {
            let _ = write_atomic(&path, &text);
        }
    }

    pub fn reload(&mut self) {
        self.flights.clear();
        self.load_errors = 0;
        let dir = self.flights_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str::<FlightRecord>(&text).ok())
            {
                Some(record) => self.flights.push(record),
                None => self.load_errors += 1,
            }
        }
        self.flights.sort_by(|a, b| b.started.cmp(&a.started));
    }

    pub fn add(&mut self, record: FlightRecord) -> Result<PathBuf, String> {
        let path = self.flights_dir().join(format!("{}.json", record.id));
        let text = serde_json::to_string(&record).map_err(|e| e.to_string())?;
        write_atomic(&path, &text)?;
        self.flights.retain(|f| f.id != record.id);
        self.flights.push(record);
        self.flights.sort_by(|a, b| b.started.cmp(&a.started));
        Ok(path)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let path = self.flights_dir().join(format!("{id}.json"));
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        self.flights.retain(|f| f.id != id);
        Ok(())
    }

    pub fn clear_all(&mut self) -> Result<usize, String> {
        let mut removed = 0;
        if let Ok(entries) = std::fs::read_dir(self.flights_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json")
                    && std::fs::remove_file(&path).is_ok()
                {
                    removed += 1;
                }
            }
        }
        self.flights.clear();
        Ok(removed)
    }

    pub fn find(&self, id: &str) -> Option<&FlightRecord> {
        self.flights.iter().find(|f| f.id == id)
    }

    pub fn stats(&self) -> Stats {
        let mut stats = Stats::default();
        stats.flights = self.flights.len();
        if self.flights.is_empty() {
            return stats;
        }
        let mut fpm_sum = 0.0;
        let mut best = f64::MAX;
        let mut worst = f64::MIN;
        let mut aircraft: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for flight in &self.flights {
            stats.total_hours += flight.duration_s / 3600.0;
            stats.total_nm += flight.distance_nm;
            stats.avg_duration_s += flight.duration_s;
            if let Some(landing) = &flight.landing {
                let rate = landing.fpm.abs();
                if rate.is_finite() && rate > 0.0 {
                    fpm_sum += rate;
                    stats.landings += 1;
                    best = best.min(rate);
                    worst = worst.max(rate);
                }
            }
            let name = if flight.aircraft.is_empty() {
                "Unknown".to_string()
            } else {
                flight.aircraft.clone()
            };
            *aircraft.entry(name).or_insert(0) += 1;
        }

        if stats.landings > 0 {
            stats.avg_landing_fpm = fpm_sum / stats.landings as f64;
            stats.best_landing_fpm = best;
            stats.worst_landing_fpm = worst;
        }
        stats.avg_duration_s /= stats.flights as f64;
        if let Some((name, count)) = aircraft.into_iter().max_by_key(|(_, c)| *c) {
            stats.top_aircraft = name;
            stats.top_aircraft_count = count;
        }
        stats
    }

    pub fn export_csv(&self) -> Result<PathBuf, String> {
        let path = self.exports_dir().join(format!("logbook-{}.csv", now_epoch()));
        let mut out = String::from(
            "id,started_utc,aircraft,origin,destination,duration_s,distance_nm,max_alt_ft,max_gs_kt,landing_fpm,landing_g,rating\n",
        );
        for f in &self.flights {
            let (fpm, g, rating) = match &f.landing {
                Some(l) => (l.fpm.abs(), l.g, l.rating.clone()),
                None => (f64::NAN, f64::NAN, String::new()),
            };
            out.push_str(&format!(
                "{},{},{},{},{},{:.0},{:.1},{:.0},{:.0},{},{},{}\n",
                f.id,
                fmt_utc_full(f.started),
                csv_escape(&f.aircraft),
                csv_escape(&f.origin),
                csv_escape(&f.dest),
                f.duration_s,
                f.distance_nm,
                f.max_alt_ft,
                f.max_gs_kt,
                if fpm.is_finite() { format!("{fpm:.0}") } else { String::new() },
                if g.is_finite() { format!("{g:.2}") } else { String::new() },
                rating,
            ));
        }
        write_atomic(&path, &out)?;
        Ok(path)
    }

    pub fn export_gpx(&self, record: &FlightRecord) -> Result<PathBuf, String> {
        let path = self.exports_dir().join(format!("flight-{}.gpx", record.id));
        let mut out = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"MSFS Flight Tracker\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n",
        );
        out.push_str(&format!(
            "  <metadata><name>{}</name><time>{}</time></metadata>\n",
            xml_escape(&record.route_label()),
            iso8601(record.started)
        ));
        out.push_str(&format!(
            "  <trk><name>{}</name><trkseg>\n",
            xml_escape(&record.route_label())
        ));
        for point in &record.track {
            out.push_str(&format!(
                "    <trkpt lat=\"{:.6}\" lon=\"{:.6}\"><ele>{:.1}</ele><time>{}</time></trkpt>\n",
                point.lat,
                point.lon,
                point.alt * 0.3048,
                iso8601(record.started + point.t.max(0.0) as u64)
            ));
        }
        out.push_str("  </trkseg></trk>\n</gpx>\n");
        write_atomic(&path, &out)?;
        Ok(path)
    }

    pub fn export_kml(&self, record: &FlightRecord) -> Result<PathBuf, String> {
        let path = self.exports_dir().join(format!("flight-{}.kml", record.id));
        let mut out = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n<Document>\n",
        );
        out.push_str(&format!(
            "  <name>{}</name>\n  <Placemark>\n    <name>{}</name>\n    <LineString>\n      <altitudeMode>absolute</altitudeMode>\n      <coordinates>\n",
            xml_escape(&record.route_label()),
            xml_escape(&fmt_utc_full(record.started))
        ));
        for point in &record.track {
            out.push_str(&format!(
                "        {:.6},{:.6},{:.1}\n",
                point.lon,
                point.lat,
                point.alt * 0.3048
            ));
        }
        out.push_str("      </coordinates>\n    </LineString>\n  </Placemark>\n</Document>\n</kml>\n");
        write_atomic(&path, &out)?;
        Ok(path)
    }
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn iso8601(epoch: u64) -> String {
    let days = (epoch / 86_400) as i64;
    let secs = epoch % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year,
        m,
        d,
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}
