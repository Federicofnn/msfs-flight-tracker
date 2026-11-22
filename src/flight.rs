//! Flight recording: automatic takeoff/landing detection, touchdown analysis
//! and the persisted flight record.

use crate::plan::FlightPlan;
use crate::sim::Telemetry;
use crate::util::{haversine_nm, now_epoch};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TrackPoint {
    pub t: f64,
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
    pub gs: f64,
    pub vs: f64,
    pub fuel: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Landing {
    pub fpm: f64,
    pub g: f64,
    pub ias: f64,
    pub pitch: f64,
    pub bank: f64,
    pub hdg: f64,
    pub wind_dir: f64,
    pub wind_kt: f64,
    pub lat: f64,
    pub lon: f64,
    pub rating: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct FlightRecord {
    pub id: String,
    pub started: u64,
    pub ended: u64,
    pub duration_s: f64,
    pub aircraft: String,
    pub origin: String,
    pub dest: String,
    pub plan_title: String,
    pub distance_nm: f64,
    pub max_alt_ft: f64,
    pub max_gs_kt: f64,
    pub fuel_start_kg: f64,
    pub fuel_end_kg: f64,
    #[serde(default)]
    pub landing: Option<Landing>,
    #[serde(default)]
    pub track: Vec<TrackPoint>,
}

impl FlightRecord {
    pub fn route_label(&self) -> String {
        match (self.origin.is_empty(), self.dest.is_empty()) {
            (false, false) => format!("{} - {}", self.origin, self.dest),
            _ => "Free flight".to_string(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Ground,
    Airborne,
    Rollout,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Idle => "IDLE",
            Phase::Ground => "ON GROUND",
            Phase::Airborne => "AIRBORNE",
            Phase::Rollout => "LANDED",
        }
    }
}

fn sane(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

pub fn rating_for(fpm: f64, g: f64) -> &'static str {
    let rate = fpm.abs();
    let base = if rate <= 50.0 {
        "BUTTER"
    } else if rate <= 100.0 {
        "SMOOTH"
    } else if rate <= 150.0 {
        "GOOD"
    } else if rate <= 250.0 {
        "FIRM"
    } else if rate <= 450.0 {
        "HARD"
    } else if rate <= 800.0 {
        "VERY HARD"
    } else {
        "CRASH"
    };
    if g >= 2.2 {
        return if rate > 450.0 { "VERY HARD" } else { "HARD" };
    }
    base
}

pub struct Recorder {
    pub active: bool,
    /// Set when the pilot stopped the flight by hand: the automatic recorder
    /// must not restart on its own until they start the next one.
    pub manual_hold: bool,
    pub phase: Phase,
    pub landing: Option<Landing>,
    pub track: Vec<TrackPoint>,
    pub distance_nm: f64,
    pub max_alt_ft: f64,
    pub max_gs_kt: f64,
    pub fuel_start_kg: f64,
    aircraft: String,
    origin: String,
    dest: String,
    plan_title: String,
    started_epoch: u64,
    started: Option<Instant>,
    touchdown_at: Option<Instant>,
    last_sample: Option<Instant>,
    last_pos: Option<(f64, f64)>,
    prev_on_ground: Option<bool>,
    prev_vs: f64,
    prev_ias: f64,
    g_peak: f64,
    recorded_points: usize,
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            active: false,
            manual_hold: false,
            phase: Phase::Idle,
            landing: None,
            track: Vec::new(),
            distance_nm: 0.0,
            max_alt_ft: 0.0,
            max_gs_kt: 0.0,
            fuel_start_kg: f64::NAN,
            aircraft: String::new(),
            origin: String::new(),
            dest: String::new(),
            plan_title: String::new(),
            started_epoch: 0,
            started: None,
            touchdown_at: None,
            last_sample: None,
            last_pos: None,
            prev_on_ground: None,
            prev_vs: 0.0,
            prev_ias: 0.0,
            g_peak: 0.0,
            recorded_points: 0,
        }
    }

    pub fn elapsed(&self) -> f64 {
        self.started.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0)
    }

    pub fn start(&mut self, t: &Telemetry, plan: Option<&FlightPlan>) {
        self.active = true;
        self.phase = if t.on_ground { Phase::Ground } else { Phase::Airborne };
        self.landing = None;
        self.track.clear();
        self.distance_nm = 0.0;
        self.max_alt_ft = if t.alt_ft.is_finite() { t.alt_ft } else { 0.0 };
        self.max_gs_kt = if t.gs_kt.is_finite() { t.gs_kt } else { 0.0 };
        self.fuel_start_kg = t.fuel_kg;
        self.aircraft = if t.title.is_empty() { t.atc_model.clone() } else { t.title.clone() };
        if self.aircraft.is_empty() {
            self.aircraft = "Unknown aircraft".to_string();
        }
        self.origin = plan.map(|p| p.origin.clone()).unwrap_or_default();
        self.dest = plan.map(|p| p.dest.clone()).unwrap_or_default();
        self.plan_title = plan.map(|p| p.title.clone()).unwrap_or_default();
        self.started_epoch = now_epoch();
        self.started = Some(Instant::now());
        self.touchdown_at = None;
        self.last_sample = None;
        self.last_pos = None;
        self.prev_on_ground = Some(t.on_ground);
        self.prev_vs = t.vs_fpm;
        self.prev_ias = t.ias_kt;
        self.g_peak = 0.0;
        self.recorded_points = 0;
        self.push_point(t, 0.0);
    }

    fn push_point(&mut self, t: &Telemetry, elapsed: f64) {
        if !t.position_valid() {
            return;
        }
        if self.recorded_points >= 30_000 {
            return; // cap memory on very long sessions
        }
        self.track.push(TrackPoint {
            t: sane(elapsed),
            lat: sane(t.lat),
            lon: sane(t.lon),
            alt: sane(t.alt_ft),
            gs: sane(t.gs_kt),
            vs: sane(t.vs_fpm),
            fuel: sane(t.fuel_kg),
        });
        self.recorded_points += 1;
        self.last_sample = Some(Instant::now());
    }

    /// Feed one telemetry frame. Returns a finished record when the flight ends.
    pub fn observe(
        &mut self,
        t: &Telemetry,
        plan: Option<&FlightPlan>,
        auto_record: bool,
        sample_secs: f64,
    ) -> Option<FlightRecord> {
        if !self.active {
            self.phase = if t.on_ground { Phase::Ground } else { Phase::Idle };
            if auto_record && !self.manual_hold && !t.on_ground && t.agl_ft > 30.0 && t.gs_kt > 40.0 {
                self.start(t, plan);
            }
            self.prev_on_ground = Some(t.on_ground);
            return None;
        }

        let elapsed = self.elapsed();

        if t.position_valid() {
            if let Some(last) = self.last_pos {
                let step = haversine_nm(last.0, last.1, t.lat, t.lon);
                if step < 5.0 {
                    self.distance_nm += step;
                }
            }
            self.last_pos = Some((t.lat, t.lon));
        }

        if t.alt_ft.is_finite() {
            self.max_alt_ft = self.max_alt_ft.max(t.alt_ft);
        }
        if t.gs_kt.is_finite() {
            self.max_gs_kt = self.max_gs_kt.max(t.gs_kt);
        }
        if t.g_force.is_finite() {
            self.g_peak = self.g_peak.max(t.g_force);
        }

        let sample_due = self
            .last_sample
            .map(|s| s.elapsed().as_secs_f64() >= sample_secs.max(0.5))
            .unwrap_or(true);
        if sample_due && !t.on_ground {
            self.push_point(t, elapsed);
        } else if sample_due && self.touchdown_at.is_some() {
            self.push_point(t, elapsed);
        }

        let was_airborne = self.prev_on_ground == Some(false);
        if was_airborne && t.on_ground && self.touchdown_at.is_none() {
            self.landing = Some(self.analyse_touchdown(t));
            self.touchdown_at = Some(Instant::now());
            self.phase = Phase::Rollout;
            self.push_point(t, elapsed);
        } else if !t.on_ground {
            self.phase = Phase::Airborne;
        } else if self.touchdown_at.is_none() && self.phase == Phase::Idle {
            self.phase = Phase::Ground;
        }

        self.prev_on_ground = Some(t.on_ground);
        self.prev_vs = t.vs_fpm;
        if t.ias_kt.is_finite() && t.ias_kt > 1.0 {
            self.prev_ias = t.ias_kt;
        }

        if let Some(at) = self.touchdown_at {
            let settled = t.gs_kt < 3.0 && at.elapsed().as_secs_f64() > 6.0;
            let timeout = at.elapsed().as_secs_f64() > 180.0;
            if settled || timeout {
                return self.finalize();
            }
        }
        None
    }

    fn analyse_touchdown(&self, t: &Telemetry) -> Landing {
        let td_fps = if t.td_vs_fps.is_finite() && t.td_vs_fps.abs() > 0.05 && t.td_vs_fps.abs() < 45.0
        {
            t.td_vs_fps
        } else {
            self.prev_vs / 60.0
        };
        let fpm = -td_fps.abs() * 60.0;
        let g = if self.g_peak.is_finite() && self.g_peak > 0.3 {
            self.g_peak.min(4.0)
        } else if t.g_force.is_finite() {
            t.g_force
        } else {
            f64::NAN
        };
        let ias = if self.prev_ias > 20.0 { self.prev_ias } else { t.ias_kt };
        // The aircraft position at the touchdown frame is unambiguous, unlike the
        // PLANE TOUCHDOWN LAT/LON datums whose native unit is radians.
        let rating = rating_for(fpm, g).to_string();
        Landing {
            fpm: sane(fpm),
            g: sane(g),
            ias: sane(ias),
            pitch: sane(if t.td_pitch_deg.is_finite() { t.td_pitch_deg } else { t.pitch_deg }),
            bank: sane(if t.td_bank_deg.is_finite() { t.td_bank_deg } else { t.bank_deg }),
            hdg: sane(if t.td_hdg_deg.is_finite() { t.td_hdg_deg } else { t.hdg_true }),
            wind_dir: sane(t.wind_dir),
            wind_kt: sane(t.wind_kt),
            lat: sane(t.lat),
            lon: sane(t.lon),
            rating,
        }
    }

    pub fn finalize(&mut self) -> Option<FlightRecord> {
        if !self.active {
            return None;
        }
        let elapsed = self.elapsed();
        let end_epoch = self
            .touchdown_at
            .map(|at| self.started_epoch + at.elapsed().as_secs() as u64)
            .unwrap_or_else(now_epoch);
        let ended = if end_epoch >= self.started_epoch {
            end_epoch
        } else {
            now_epoch()
        };
        let duration = if self.touchdown_at.is_some() {
            ended.saturating_sub(self.started_epoch) as f64
        } else {
            elapsed
        };

        let record = FlightRecord {
            id: format!("{}", self.started_epoch * 1000 + (self.recorded_points % 1000) as u64),
            started: self.started_epoch,
            ended,
            duration_s: sane(duration),
            aircraft: self.aircraft.clone(),
            origin: self.origin.clone(),
            dest: self.dest.clone(),
            plan_title: self.plan_title.clone(),
            distance_nm: sane(self.distance_nm),
            max_alt_ft: sane(self.max_alt_ft),
            max_gs_kt: sane(self.max_gs_kt),
            fuel_start_kg: sane(self.fuel_start_kg),
            fuel_end_kg: sane(self.track.last().map(|p| p.fuel).unwrap_or(f64::NAN)),
            landing: self.landing.clone(),
            track: self.track.clone(),
        };

        self.active = false;
        self.phase = Phase::Ground;
        self.touchdown_at = None;
        Some(record)
    }

    /// Manual stop; a very short hop without a landing is discarded.
    pub fn stop(&mut self) -> Option<FlightRecord> {
        if !self.active {
            return None;
        }
        if self.elapsed() < 60.0 && self.landing.is_none() {
            self.active = false;
            self.phase = Phase::Idle;
            self.track.clear();
            return None;
        }
        self.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratings() {
        assert_eq!(rating_for(-42.0, 1.1), "BUTTER");
        assert_eq!(rating_for(-130.0, 1.3), "GOOD");
        assert_eq!(rating_for(-700.0, 1.9), "VERY HARD");
        assert_eq!(rating_for(-180.0, 2.4), "HARD");
    }

    fn telem(on_ground: bool, agl: f64, gs: f64) -> Telemetry {
        Telemetry {
            lat: 41.8,
            lon: 12.25,
            alt_ft: 1000.0,
            agl_ft: agl,
            gs_kt: gs,
            on_ground,
            ..Default::default()
        }
    }

    #[test]
    fn a_manual_stop_holds_the_auto_recorder() {
        let mut rec = Recorder::new();
        rec.start(&telem(false, 1000.0, 200.0), None);
        assert!(rec.active);
        assert!(rec.finalize().is_some());
        assert!(!rec.active);

        // After STOP the recorder must stay idle even while airborne.
        rec.manual_hold = true;
        rec.observe(&telem(false, 1000.0, 200.0), None, true, 2.0);
        assert!(!rec.active);

        // START clears the hold and the next takeoff records again.
        rec.manual_hold = false;
        rec.observe(&telem(false, 1000.0, 200.0), None, true, 2.0);
        assert!(rec.active);
    }

    #[test]
    fn records_a_short_flight() {
        let mut rec = Recorder::new();
        assert!(rec.observe(&telem(true, 0.0, 5.0), None, true, 2.0).is_none());
        assert!(!rec.active);
        rec.observe(&telem(false, 200.0, 120.0), None, true, 2.0);
        assert!(rec.active);
        assert!(rec.observe(&telem(true, 0.0, 40.0), None, true, 2.0).is_none());
        assert!(rec.landing.is_some());
        let record = rec.stop().unwrap();
        assert!(record.duration_s >= 0.0);
        assert_eq!(record.route_label(), "Free flight");
    }
}
