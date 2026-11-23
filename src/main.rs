#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cloud;
mod flight;
mod net;
mod pages;
mod photo;
mod plan;
mod sim;
mod simbrief;
mod store;
mod theme;
mod traffic;
mod tray;
mod update;
mod util;
mod widgets;
mod worldmap;

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui;
use flight::{FlightRecord, Landing, Recorder};
use plan::FlightPlan;
use sim::{LinkState, SimEvent, Telemetry};
use store::Store;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Map,
    Plan,
    Logbook,
    Stats,
    Settings,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Page::Dashboard => "Flight deck",
            Page::Map => "Map",
            Page::Plan => "Flight plan",
            Page::Logbook => "Logbook",
            Page::Stats => "Statistics",
            Page::Settings => "Settings",
        }
    }
}

enum NetJob {
    Briefing(String),
    Photo(photo::Lookup),
    Traffic(traffic::Filter),
    UpdateCheck(String),
    UpdateDownload(update::Release),
    CloudPush {
        token: String,
        gist: String,
        flights: Vec<FlightRecord>,
    },
    CloudPull {
        token: String,
        gist: String,
    },
}

enum NetDone {
    Briefing(Result<(FlightPlan, simbrief::Briefing), String>),
    Photo(Result<(egui::ColorImage, &'static str, String, String), String>),
    Traffic(traffic::Fetched),
    UpdateCheck(Result<Option<update::Release>, String>),
    UpdateDownload(Result<std::path::PathBuf, String>),
    CloudPush(Result<String, String>),
    CloudPull(Result<Vec<FlightRecord>, String>),
}

/// Single worker thread for every network request (SimBrief, aircraft photos).
fn spawn_net() -> (Sender<NetJob>, Receiver<NetDone>) {
    let (job_tx, job_rx) = channel::<NetJob>();
    let (done_tx, done_rx) = channel::<NetDone>();
    std::thread::spawn(move || {
        while let Ok(job) = job_rx.recv() {
            match job {
                NetJob::Briefing(user) => {
                    let result = simbrief::fetch(&user);
                    let _ = done_tx.send(NetDone::Briefing(result));
                }
                NetJob::Photo(lookup) => {
                    let result = photo::fetch_photo(&lookup).and_then(|photo| {
                        photo::decode(&photo.bytes)
                            .map(|image| (image, photo.source, photo.label, photo.credit))
                    });
                    let _ = done_tx.send(NetDone::Photo(result));
                }
                NetJob::Traffic(filter) => {
                    let _ = done_tx.send(NetDone::Traffic(traffic::fetch(&filter)));
                }
                NetJob::UpdateCheck(url) => {
                    let _ = done_tx.send(NetDone::UpdateCheck(update::check(&url)));
                }
                NetJob::CloudPush {
                    token,
                    gist,
                    flights,
                } => {
                    let result = cloud::push(&token, &gist, &flights);
                    let _ = done_tx.send(NetDone::CloudPush(result));
                }
                NetJob::CloudPull { token, gist } => {
                    let result = cloud::pull(&token, &gist);
                    let _ = done_tx.send(NetDone::CloudPull(result));
                }
                NetJob::UpdateDownload(release) => {
                    let _ = done_tx.send(NetDone::UpdateDownload(update::download(&release)));
                }
            }
        }
    });
    (job_tx, done_rx)
}

pub struct App {
    rx: Receiver<SimEvent>,
    net_tx: Sender<NetJob>,
    net_rx: Receiver<NetDone>,
    pub map: worldmap::MapState,
    pub world: worldmap::World,
    pub briefing: Option<simbrief::Briefing>,
    pub simbrief_busy: bool,
    pub photo: Option<egui::TextureHandle>,
    pub photo_status: String,
    pub photo_source: &'static str,
    pub photo_label: String,
    pub photo_credit: String,
    pub confirm_stop: bool,
    pub allow_exit: bool,
    tray: Option<tray_icon::TrayIcon>,
    tray_tried: bool,
    pub cloud_status: String,
    pub cloud_error: String,
    pub cloud_busy: bool,
    photo_for: String,
    pub traffic: traffic::Traffic,
    pub traffic_filter: traffic::Filter,
    traffic_next: Instant,
    pub update_state: update::State,
    update_next: Instant,
    pub map_search: String,
    pub map_filter_tab: u8,
    pub tele: Telemetry,
    has_tele: bool,
    pub last_rx: Instant,
    pub link: LinkState,
    pub link_detail: String,
    pub sim_warning: Option<String>,
    pub plan: Option<FlightPlan>,
    pub plan_error: Option<String>,
    pub recorder: Recorder,
    pub store: Store,
    pub page: Page,
    pub selected: Option<String>,
    pub scrub: f64,
    pub playing: bool,
    pub toast: Option<(String, Instant, bool)>,
    pub confirm_clear: bool,
    pub confirm_delete: Option<String>,
    pub demo: bool,
    pub gs_smooth: f64,
    fuel_mark: Option<(f64, Instant)>,
    pub burn_kg_h: f64,
    pub landing_popup: Option<(Landing, Instant)>,
    pub popup_dismissed: bool,
}

impl App {
    fn new(demo: bool) -> Self {
        let (tx, rx) = channel();
        if demo {
            spawn_demo(tx);
        } else {
            sim::spawn(tx);
        }

        let (net_tx, net_rx) = spawn_net();
        let store = Store::open(demo);
        let mut app = App {
            rx,
            net_tx,
            net_rx,
            map: worldmap::MapState::default(),
            world: worldmap::World::embedded(),
            briefing: None,
            simbrief_busy: false,
            photo: None,
            photo_status: String::new(),
            photo_source: "",
            photo_label: String::new(),
            photo_credit: String::new(),
            confirm_stop: false,
            allow_exit: false,
            tray: None,
            tray_tried: false,
            cloud_status: String::new(),
            cloud_error: String::new(),
            cloud_busy: false,
            photo_for: String::new(),
            traffic: traffic::Traffic::default(),
            traffic_filter: traffic::Filter::default(),
            traffic_next: Instant::now(),
            update_state: update::State::default(),
            update_next: Instant::now(),
            map_search: String::new(),
            map_filter_tab: std::env::var("MSFS_TRACKER_TAB")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            tele: Telemetry::default(),
            has_tele: false,
            last_rx: Instant::now() - Duration::from_secs(10),
            link: LinkState::Starting,
            link_detail: String::new(),
            sim_warning: None,
            plan: None,
            plan_error: None,
            recorder: Recorder::new(),
            store,
            page: match std::env::var("MSFS_TRACKER_PAGE")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "map" => Page::Map,
                "plan" => Page::Plan,
                "logbook" => Page::Logbook,
                "stats" => Page::Stats,
                "settings" => Page::Settings,
                _ => Page::Map,
            },
            selected: None,
            scrub: 1.0,
            playing: false,
            toast: None,
            confirm_clear: false,
            confirm_delete: None,
            demo,
            gs_smooth: 0.0,
            fuel_mark: None,
            burn_kg_h: 0.0,
            landing_popup: None,
            popup_dismissed: false,
        };

        if demo {
            app.plan = demo_plan();
        } else if let Some(path) = app.store.settings.last_plan.clone() {
            if let Ok(plan) = plan::read_plan(std::path::Path::new(&path)) {
                app.plan = Some(plan);
            }
        }
        app
    }

    pub fn fresh(&self) -> bool {
        self.has_tele && self.last_rx.elapsed() < Duration::from_secs(4)
    }

    pub fn alt_val(&self, feet: f64) -> f64 {
        if self.store.settings.imperial {
            feet
        } else {
            feet * 0.3048
        }
    }

    pub fn alt_unit(&self) -> &'static str {
        if self.store.settings.imperial {
            "ft"
        } else {
            "m"
        }
    }

    pub fn vs_val(&self, fpm: f64) -> f64 {
        if self.store.settings.imperial {
            fpm
        } else {
            fpm * 0.00508
        }
    }

    pub fn vs_unit(&self) -> &'static str {
        if self.store.settings.imperial {
            "fpm"
        } else {
            "m/s"
        }
    }

    pub fn fuel_val(&self, kg: f64) -> f64 {
        if self.store.settings.imperial {
            kg / 0.453_592_37
        } else {
            kg
        }
    }

    pub fn fuel_unit(&self) -> &'static str {
        if self.store.settings.imperial {
            "lb"
        } else {
            "kg"
        }
    }

    /// Downloads the SimBrief plan of the pilot saved in settings.
    pub fn load_simbrief(&mut self) {
        if self.simbrief_busy {
            return;
        }
        let user = self.store.settings.simbrief_id.trim().to_string();
        if user.is_empty() {
            self.toast_err("Set your SimBrief username or pilot ID in Settings first");
            return;
        }
        self.simbrief_busy = true;
        if self.net_tx.send(NetJob::Briefing(user)).is_err() {
            self.simbrief_busy = false;
            self.toast_err("Network worker unavailable");
        }
    }

    /// Everything the photo lookup needs: tail number, airline, type and title.
    pub fn photo_lookup(&self) -> photo::Lookup {
        let mut lookup = photo::Lookup::default();
        if let Some(briefing) = &self.briefing {
            lookup.registration = briefing.registration.clone();
            lookup.airline = briefing.airline.clone();
            lookup.model = if briefing.aircraft_name.trim().is_empty() {
                briefing.aircraft_type.clone()
            } else {
                briefing.aircraft_name.clone()
            };
            lookup.title = format!("{} {}", briefing.airline, lookup.model)
                .trim()
                .to_string();
        }
        if lookup.registration.trim().is_empty() {
            lookup.registration = self.tele.atc_id.trim().to_string();
        }
        if lookup.model.trim().is_empty() {
            lookup.model = if self.tele.atc_model.trim().is_empty() {
                self.tele.title.trim().to_string()
            } else {
                self.tele.atc_model.trim().to_string()
            };
        }
        lookup.callsign = if let Some(briefing) = &self.briefing {
            briefing.callsign.clone()
        } else {
            let airline = self.tele.atc_airline.trim();
            let flight = self.tele.atc_flight.trim();
            format!("{airline}{flight}").trim().to_string()
        };
        if lookup.title.trim().is_empty() {
            lookup.title = self.tele.title.trim().to_string();
        }
        lookup.query = self.store.settings.photo_query.trim().to_string();
        lookup
    }

    /// Starts a flight by hand, whatever the aircraft is doing.
    pub fn start_flight(&mut self) {
        if self.recorder.active {
            self.toast_msg("A flight is already recording");
            return;
        }
        if !self.fresh() && self.briefing.is_none() {
            self.toast_err("No simulator data yet: start a flight in MSFS first");
            return;
        }
        self.recorder.manual_hold = false;
        let telemetry = self.tele.clone();
        self.recorder.start(&telemetry, self.plan.as_ref());
        self.toast_msg("Flight recording started");
    }

    /// Stops the current flight and saves it in the logbook.
    pub fn stop_flight(&mut self) {
        if !self.recorder.active {
            self.toast_msg("No flight is recording");
            return;
        }
        self.recorder.manual_hold = true;
        match self.recorder.finalize() {
            Some(record) => {
                let label = record.route_label();
                self.persist(record);
                self.toast_msg(format!("Flight saved · {label}"));
            }
            None => self.toast_msg("Flight discarded"),
        }
    }

    pub fn toggle_flight(&mut self) {
        if self.recorder.active {
            self.confirm_stop = true;
        } else {
            self.start_flight();
        }
    }

    pub fn traffic_changed(&mut self) {
        self.traffic_next = Instant::now();
    }

    pub fn request_traffic(&mut self) {
        if self.traffic.loading || !self.traffic_filter.active() {
            return;
        }
        self.traffic.loading = true;
        self.traffic_next = Instant::now() + Duration::from_secs(120);
        let _ = self.net_tx.send(NetJob::Traffic(self.traffic_filter.clone()));
    }

    pub fn push_cloud(&mut self, background: bool) {
        if self.cloud_busy {
            return;
        }
        self.cloud_busy = true;
        self.cloud_error.clear();
        self.cloud_status = if background {
            "Saving the logbook to GitHub…".to_string()
        } else {
            "Uploading the logbook…".to_string()
        };
        let token = self.store.settings.github_token.trim().to_string();
        let gist = self.store.settings.github_gist.trim().to_string();
        let flights = self.store.flights.clone();
        let job = NetJob::CloudPush {
            token,
            gist,
            flights,
        };
        if self.net_tx.send(job).is_err() {
            self.cloud_busy = false;
            self.cloud_error = "The network worker is not running".to_string();
        }
    }

    pub fn pull_cloud(&mut self) {
        if self.cloud_busy {
            return;
        }
        self.cloud_busy = true;
        self.cloud_error.clear();
        self.cloud_status = "Downloading the logbook…".to_string();
        let job = NetJob::CloudPull {
            token: self.store.settings.github_token.trim().to_string(),
            gist: self.store.settings.github_gist.trim().to_string(),
        };
        if self.net_tx.send(job).is_err() {
            self.cloud_busy = false;
            self.cloud_error = "The network worker is not running".to_string();
        }
    }

    pub fn open_site(&self) {
        // The code in the link means the site opens the logbook straight away.
        let gist = self.store.settings.github_gist.trim();
        let url = if gist.is_empty() {
            "https://federicofnn.github.io/msfs-flight-tracker/".to_string()
        } else {
            format!("https://federicofnn.github.io/msfs-flight-tracker/#{gist}")
        };
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn();
    }

    pub fn check_updates(&mut self, manual: bool) {
        let configured = self.store.settings.update_url.trim().to_string();
        let url = if configured.is_empty() {
            update::DEFAULT_MANIFEST.to_string()
        } else {
            configured
        };
        if url.is_empty() {
            if manual {
                self.toast_err("Set the update manifest address in Settings first");
            }
            return;
        }
        if self.update_state.checking {
            return;
        }
        self.update_state.checking = true;
        self.update_state.error.clear();
        self.update_state.notes.clear();
        self.update_next = Instant::now() + Duration::from_secs(6 * 60 * 60);
        let _ = self.net_tx.send(NetJob::UpdateCheck(url));
    }

    pub fn install_update(&mut self) {
        let Some(release) = self.update_state.available.clone() else {
            return;
        };
        if self.update_state.installer.is_some() || self.update_state.downloading {
            return;
        }
        self.update_state.downloading = true;
        let _ = self.net_tx.send(NetJob::UpdateDownload(release));
    }

    pub fn request_photo(&mut self, lookup: &photo::Lookup, ctx: &egui::Context) {
        if lookup.is_empty() {
            return;
        }
        let key = lookup.key();
        if self.photo_for == key {
            return;
        }
        self.photo_for = key;
        self.photo_status = "loading".to_string();
        let _ = self.net_tx.send(NetJob::Photo(lookup.clone()));
        ctx.request_repaint();
    }

    pub fn retry_photo(&mut self, lookup: &photo::Lookup, ctx: &egui::Context) {
        self.photo_for.clear();
        self.photo_status.clear();
        self.request_photo(lookup, ctx);
    }

    /// Grabs the photo once: by registration when known, by type otherwise.
    fn auto_photo(&mut self, ctx: &egui::Context) {
        if self.photo.is_some() || !self.photo_for.is_empty() {
            return;
        }
        if !self.fresh() && self.briefing.is_none() {
            return;
        }
        let lookup = self.photo_lookup();
        if lookup.is_empty() {
            return;
        }
        self.request_photo(&lookup, ctx);
    }

    pub fn toast_msg(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now(), false));
    }

    pub fn toast_err(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now(), true));
    }

    fn on_event(&mut self, event: SimEvent) {
        match event {
            SimEvent::Telemetry(t) => {
                let dt = self.last_rx.elapsed().as_secs_f64().clamp(0.01, 5.0);
                if t.gs_kt.is_finite() {
                    if self.gs_smooth <= 0.0 {
                        self.gs_smooth = t.gs_kt;
                    } else {
                        let k = (dt / 7.0).clamp(0.02, 0.4);
                        self.gs_smooth += (t.gs_kt - self.gs_smooth) * k;
                    }
                }

                if t.fuel_kg.is_finite() {
                    match self.fuel_mark {
                        Some((fuel, when)) => {
                            let secs = when.elapsed().as_secs_f64();
                            if secs >= 30.0 {
                                let used = fuel - t.fuel_kg;
                                if used > 0.0 {
                                    let rate = used / (secs / 3600.0);
                                    self.burn_kg_h = if self.burn_kg_h <= 0.0 {
                                        rate
                                    } else {
                                        self.burn_kg_h * 0.7 + rate * 0.3
                                    };
                                }
                                self.fuel_mark = Some((t.fuel_kg, Instant::now()));
                            }
                        }
                        None => self.fuel_mark = Some((t.fuel_kg, Instant::now())),
                    }
                }

                self.tele = t;
                self.last_rx = Instant::now();
                self.has_tele = true;

                let auto = self.store.settings.auto_record;
                let sample = self.store.settings.sample_secs;
                let finished = self
                    .recorder
                    .observe(&self.tele, self.plan.as_ref(), auto, sample);

                if let (Some(landing), None) = (&self.recorder.landing, &self.landing_popup) {
                    self.landing_popup = Some((landing.clone(), Instant::now()));
                    self.popup_dismissed = false;
                }

                if let Some(record) = finished {
                    self.persist(record);
                }
            }
            SimEvent::Status(status) => {
                self.link = status.state;
                self.link_detail = status.detail;
            }
            SimEvent::Log(message) => {
                if !message.is_empty() {
                    self.sim_warning = Some(message);
                }
            }
        }
    }

    fn persist(&mut self, record: FlightRecord) {
        let label = format!(
            "{} · {} · {}",
            record.route_label(),
            util::fmt_duration(record.duration_s),
            match &record.landing {
                Some(l) => format!("{:.0} fpm", l.fpm.abs()),
                None => "no touchdown".to_string(),
            }
        );
        match self.store.add(record) {
            Ok(_) => {
                self.toast_msg(format!("Flight saved — {label}"));
                if self.store.settings.sync_auto
                    && !self.store.settings.github_token.trim().is_empty()
                {
                    self.push_cloud(true);
                }
            }
            Err(err) => self.toast_err(format!("Could not save the flight: {err}")),
        }
    }

    /// Without live telemetry the map jumps to the planned route.
    fn focus_plan(&mut self) {
        if self.fresh() {
            self.map.follow = true;
            return;
        }
        if let Some(plan) = &self.plan {
            if let (Some(from), Some(to)) = (plan.origin_pos, plan.dest_pos) {
                self.map.center_on((from.0 + to.0) * 0.5, (from.1 + to.1) * 0.5);
                self.map.zoom = 6.2;
                self.map.follow = false;
            }
        }
    }

    fn load_plan(&mut self, path: &std::path::Path) {
        match plan::read_plan(path) {
            Ok(loaded) => {
                let label = format!("{} → {}", loaded.origin, loaded.dest);
                self.plan = Some(loaded);
                self.plan_error = None;
                self.store.settings.last_plan = Some(path.display().to_string());
                self.store.save_settings();
                self.focus_plan();
                self.toast_msg(format!("Flight plan loaded: {label}"));
            }
            Err(err) => {
                self.plan_error = Some(err.clone());
                self.toast_err(format!("Flight plan error: {err}"));
            }
        }
    }

    pub fn pick_plan(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("MSFS flight plan", &["pln"])
            .set_title("Open a Microsoft Flight Simulator flight plan")
            .pick_file()
        {
            self.load_plan(&path);
        }
    }

    fn pump(&mut self, ctx: &egui::Context) {
        let mut processed = 0;
        while processed < 400 {
            match self.rx.try_recv() {
                Ok(event) => {
                    self.on_event(event);
                    processed += 1;
                }
                Err(_) => break,
            }
        }

        while let Ok(done) = self.net_rx.try_recv() {
            match done {
                NetDone::Briefing(result) => {
                    self.simbrief_busy = false;
                    match result {
                        Ok((plan, briefing)) => {
                            let label = format!("{} to {}", plan.origin, plan.dest);
                            self.plan = Some(plan);
                            self.plan_error = None;
                            self.store.settings.last_plan = None;
                            self.store.save_settings();
                            self.map.follow = true;
                            self.map.zoom = self.map.zoom.max(5.2);
                            self.focus_plan();
                            self.briefing = Some(briefing);
                            self.toast_msg(format!("SimBrief plan loaded · {label}"));
                            let lookup = self.photo_lookup();
                            self.request_photo(&lookup, ctx);
                        }
                        Err(err) => self.toast_err(format!("SimBrief: {err}")),
                    }
                }
                NetDone::Traffic(fetched) => {
                    self.traffic.loading = false;
                    self.traffic.pilots.clear();
                    match fetched.vatsim {
                        Ok(pilots) => {
                            self.traffic.vatsim_total = pilots.len();
                            self.traffic.vatsim_error.clear();
                            self.traffic.pilots.extend(pilots);
                        }
                        Err(err) => {
                            self.traffic.vatsim_total = 0;
                            self.traffic.vatsim_error = err;
                        }
                    }
                    match fetched.ivao {
                        Ok(pilots) => {
                            self.traffic.ivao_total = pilots.len();
                            self.traffic.ivao_error.clear();
                            self.traffic.pilots.extend(pilots);
                        }
                        Err(err) => {
                            self.traffic.ivao_total = 0;
                            self.traffic.ivao_error = err;
                        }
                    }
                    self.traffic.updated = Some(Instant::now());
                    let failed = if !self.traffic.vatsim_error.is_empty() {
                        Some(self.traffic.vatsim_error.clone())
                    } else if !self.traffic.ivao_error.is_empty() {
                        Some(self.traffic.ivao_error.clone())
                    } else {
                        None
                    };
                    if let Some(err) = failed {
                        self.toast_err(format!("Traffic: {err}"));
                    }
                }
                NetDone::UpdateCheck(result) => {
                    self.update_state.checking = false;
                    match result {
                        Ok(Some(release)) => {
                            self.update_state.status =
                                format!("Version {} is available", release.version);
                            self.update_state.notes = release.notes.clone();
                            self.update_state.available = Some(release);
                            self.toast_msg("Update available: open Settings › Updates");
                        }
                        Ok(None) => {
                            self.update_state.status = "You are up to date".to_string();
                            self.update_state.available = None;
                        }
                        Err(err) => {
                            self.update_state.error = err;
                        }
                    }
                }
                NetDone::UpdateDownload(result) => {
                    self.update_state.downloading = false;
                    match result {
                        Ok(path) => {
                            self.update_state.installer = Some(path.clone());
                            self.update_state.status =
                                "Starting the installer…".to_string();
                            match update::run_installer_ui(&path) {
                                Ok(()) => {
                                    self.allow_exit = true;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                                Err(err) => self.update_state.error = err,
                            }
                        }
                        Err(err) => self.update_state.error = err,
                    }
                }
                NetDone::CloudPush(result) => {
                    self.cloud_busy = false;
                    match result {
                        Ok(id) => {
                            if self.store.settings.github_gist != id {
                                self.store.settings.github_gist = id;
                                self.store.save_settings();
                            }
                            self.cloud_status = format!(
                                "{} flights saved on GitHub",
                                self.store.flights.len()
                            );
                        }
                        Err(err) => {
                            self.cloud_error = err.clone();
                            self.cloud_status.clear();
                            self.toast_err(format!("Cloud sync: {err}"));
                        }
                    }
                }
                NetDone::CloudPull(result) => {
                    self.cloud_busy = false;
                    match result {
                        Ok(flights) => {
                            let remote = flights.len();
                            let mut added = 0;
                            for record in flights {
                                if !self
                                    .store
                                    .flights
                                    .iter()
                                    .any(|existing| existing.id == record.id)
                                    && self.store.add(record).is_ok()
                                {
                                    added += 1;
                                }
                            }
                            self.cloud_status = format!(
                                "{added} new flights from GitHub ({remote} in the cloud)"
                            );
                        }
                        Err(err) => {
                            self.cloud_error = err.clone();
                            self.cloud_status.clear();
                            self.toast_err(format!("Cloud sync: {err}"));
                        }
                    }
                }
                NetDone::Photo(result) => match result {
                    Ok((image, source, label, credit)) => {
                        self.photo = Some(ctx.load_texture(
                            "aircraft-photo",
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                        self.photo_source = source;
                        self.photo_label = label;
                        self.photo_credit = credit;
                        self.photo_status.clear();
                    }
                    Err(err) => {
                        self.photo = None;
                        self.photo_status = err;
                    }
                },
            }
        }

        self.auto_photo(ctx);

        // Traffic is only downloaded while the map is on screen.
        if self.page == Page::Map && self.traffic_filter.active() {
            if !self.traffic.loading && Instant::now() >= self.traffic_next {
                self.request_traffic();
            }
        } else if !self.traffic_filter.active() && !self.traffic.pilots.is_empty() {
            self.traffic.reset();
        }

        if self.store.settings.check_updates && Instant::now() >= self.update_next {
            self.check_updates(false);
        }

        if let Some((_, at, _)) = &self.toast {
            if at.elapsed() > Duration::from_secs(6) {
                self.toast = None;
            }
        }
        if let Some((_, at)) = &self.landing_popup {
            if at.elapsed() > Duration::from_secs(150) {
                self.landing_popup = None;
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let files = ctx.input(|i| i.raw.dropped_files.clone());
        for file in files {
            if let Some(path) = file.path {
                if path.extension().and_then(|e| e.to_str()) == Some("pln") {
                    self.load_plan(&path);
                }
            }
        }
    }

    fn nav_row(&mut self, ui: &mut egui::Ui, page: Page) {
        let selected = self.page == page;
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 38.0), egui::Sense::click());

        if selected || response.hovered() {
            let fill = if selected {
                egui::Color32::from_rgb(40, 24, 28)
            } else {
                egui::Color32::from_rgb(30, 20, 23)
            };
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(8), fill);
        }
        if selected {
            let bar = egui::Rect::from_min_size(
                rect.left_top() + egui::vec2(0.0, 8.0),
                egui::vec2(3.0, rect.height() - 16.0),
            );
            ui.painter().rect_filled(
                bar,
                egui::CornerRadius::same(2),
                theme::ACCENT,
            );
        }
        ui.painter().text(
            rect.left_center() + egui::vec2(14.0, 0.0),
            egui::Align2::LEFT_CENTER,
            page.title(),
            egui::FontId::proportional(13.5),
            if selected { theme::TEXT } else { theme::MUTED },
        );

        if response.clicked() {
            self.page = page;
        }
    }

    fn shell(&mut self, ctx: &egui::Context) {
        // The map is full-bleed: its own floating rail and top bar replace the
        // fixed chrome, exactly like the reference dashboard.
        if self.page == Page::Map {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::BG))
                .show(ctx, |ui| self.map_page(ui));
            return;
        }

        egui::SidePanel::left("nav")
            .exact_width(202.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(12, 16)),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("MSFS")
                        .size(11.0)
                        .color(theme::ACCENT)
                        .strong(),
                );
                ui.label(
                    egui::RichText::new("FLIGHT TRACKER")
                        .size(16.0)
                        .color(theme::TEXT)
                        .strong(),
                );
                ui.add_space(20.0);
                for page in [
                    Page::Dashboard,
                    Page::Map,
                    Page::Plan,
                    Page::Logbook,
                    Page::Stats,
                    Page::Settings,
                ] {
                    self.nav_row(ui, page);
                    ui.add_space(3.0);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        egui::RichText::new(env!("CARGO_PKG_VERSION"))
                            .size(10.0)
                            .color(theme::DIM),
                    );
                    ui.label(
                        egui::RichText::new(if self.demo {
                            "demo mode"
                        } else {
                            "SimConnect"
                        })
                        .size(10.0)
                        .color(theme::DIM),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} flights logged",
                            self.store.flights.len()
                        ))
                        .size(10.5)
                        .color(theme::MUTED),
                    );
                });
            });

        egui::TopBottomPanel::top("topbar")
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(18, 10))
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let context = match &self.plan {
                        Some(plan) => format!("{} to {}", plan.origin, plan.dest),
                        None => "Free flight - no plan loaded".to_string(),
                    };
                    ui.label(
                        egui::RichText::new(context)
                            .size(12.5)
                            .color(theme::MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, color) = match self.link {
                            LinkState::Connected if self.fresh() => ("SIM CONNECTED", theme::GREEN),
                            LinkState::Connected => ("WAITING FOR DATA", theme::ACCENT),
                            LinkState::WaitingSim => ("WAITING FOR MSFS", theme::ACCENT),
                            LinkState::NoDll => ("SIMCONNECT.DLL MISSING", theme::RED),
                            LinkState::Starting => ("STARTING", theme::MUTED),
                        };
                        widgets::pill(ui, label, color);
                        if !self.fresh() && !self.link_detail.trim().is_empty() {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(short_detail(&self.link_detail))
                                    .size(10.5)
                                    .color(theme::MUTED),
                            );
                        }
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(util::fmt_zulu_hhmm(util::now_epoch()))
                                .size(12.0)
                                .family(egui::FontFamily::Monospace)
                                .color(theme::MUTED),
                        );
                        ui.add_space(8.0);
                        let (label, color) = if self.recorder.active {
                            ("STOP & SAVE", theme::RED)
                        } else {
                            ("START FLIGHT", theme::ACCENT)
                        };
                        let enabled =
                            self.recorder.active || self.fresh() || self.briefing.is_some();
                        if widgets::action_button(ui, label, color, enabled) {
                            self.toggle_flight();
                        }
                        if self.recorder.active {
                            ui.add_space(6.0);
                            widgets::pill(
                                ui,
                                &format!("REC {}", util::fmt_clock(self.recorder.elapsed())),
                                theme::RED,
                            );
                        }
                        if let Some(release) = self.update_state.available.clone() {
                            ui.add_space(6.0);
                            if widgets::action_button(
                                ui,
                                &format!("UPDATE {}", release.version),
                                theme::ACCENT,
                                !self.update_state.downloading,
                            ) {
                                self.page = Page::Settings;
                            }
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::same(18)),
            )
            .show(ctx, |ui| {
                if self.page == Page::Map {
                    self.map_page(ui);
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            match self.page {
                                Page::Dashboard => self.dashboard(ui),
                                Page::Map => {}
                                Page::Plan => self.plan_page(ui),
                                Page::Logbook => self.logbook(ui),
                                Page::Stats => self.stats_page(ui),
                                Page::Settings => self.settings_page(ui),
                            }
                        });
                }
            });
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some((message, at, is_error)) = self.toast.clone() else {
            return;
        };
        if at.elapsed() > Duration::from_secs(6) {
            return;
        }
        egui::Area::new(egui::Id::new("toast"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-20.0, -20.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(32, 18, 22))
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        if is_error { theme::RED } else { theme::CYAN },
                    ))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(message)
                                .size(12.5)
                                .color(if is_error { theme::RED } else { theme::TEXT }),
                        );
                    });
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.tray_tried {
            self.tray_tried = true;
            self.tray = tray::start(ctx);
        }
        for msg in tray::take() {
            match msg {
                tray::TrayMsg::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                tray::TrayMsg::Quit => {
                    self.allow_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if ctx.input(|input| input.viewport().close_requested()) && !self.allow_exit {
            // Closing keeps the tracker alive, so a flight in progress is not lost.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.tray.is_some() {
                // Out of the taskbar and next to the clock; the icon reopens it.
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.toast_msg("Still running next to the clock");
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                self.toast_msg("Still running in the background");
            }
        }
        self.pump(ctx);
        self.handle_dropped_files(ctx);
        self.stop_confirm(ctx);
        self.shell(ctx);
        self.toast_ui(ctx);
        ctx.request_repaint_after(Duration::from_millis(80));
    }
}

/// The link message is cut so the top bar keeps its shape.
fn short_detail(text: &str) -> String {
    const MAX: usize = 64;
    let text = text.trim();
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX).collect();
    format!("{cut}…")
}

fn demo_plan() -> Option<FlightPlan> {
    const PLN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<SimBase.Document Type="AceXML" version="1,0">
<FlightPlan.FlightPlan>
<Title>LIRF to LIRN (demo)</Title>
<CruisingAlt>35000</CruisingAlt>
<DepartureID>LIRF</DepartureID>
<DepartureLLA>N41° 48' 1.08",E12° 14' 20.04",+000013.00</DepartureLLA>
<DestinationID>LIRN</DestinationID>
<DestinationLLA>N40° 53' 9.60",E14° 17' 26.88",+0000294.00</DestinationLLA>
<ATCWaypoint id="LIRF">
<ATCWaypointType>Airport</ATCWaypointType>
<WorldPosition>N41° 48' 1.08",E12° 14' 20.04",+000013.00</WorldPosition>
<ICAO><ICAOIdent>LIRF</ICAOIdent></ICAO>
</ATCWaypoint>
<ATCWaypoint id="NEMBO">
<ATCWaypointType>Waypoint</ATCWaypointType>
<WorldPosition>N41° 26' 0.00",E12° 58' 0.00",+0000000.00</WorldPosition>
</ATCWaypoint>
<ATCWaypoint id="TEA">
<ATCWaypointType>VOR</ATCWaypointType>
<WorldPosition>N41° 10' 0.00",E13° 35' 0.00",+0000000.00</WorldPosition>
</ATCWaypoint>
<ATCWaypoint id="LIRN">
<ATCWaypointType>Airport</ATCWaypointType>
<WorldPosition>N40° 53' 9.60",E14° 17' 26.88",+0000294.00</WorldPosition>
<ICAO><ICAOIdent>LIRN</ICAOIdent></ICAO>
</ATCWaypoint>
</FlightPlan.FlightPlan>
</SimBase.Document>"#;
    FlightPlan::parse(PLN, None).ok()
}

/// Synthetic telemetry so the interface can be reviewed without the sim.
fn spawn_demo(tx: Sender<SimEvent>) {
    std::thread::spawn(move || {
        let _ = tx.send(SimEvent::Status(sim::Status {
            state: LinkState::Connected,
            detail: "Demo telemetry (no simulator attached)".to_string(),
        }));
        let start = Instant::now();
        let cycle = 260.0f64;
        loop {
            let elapsed = start.elapsed().as_secs_f64();
            let p = (elapsed % cycle) / cycle;

            let (alt, gs, vs, on_ground, agl, pitch, bank, td_vs) = if p < 0.05 {
                (13.0, 4.0 + p * 200.0, 0.0, true, 0.0, 0.0, 0.0, 0.0)
            } else if p < 0.16 {
                let k = (p - 0.05) / 0.11;
                (13.0 + k * 34_987.0, 150.0 + k * 130.0, 2400.0, false, k * 34_987.0, 12.0 * k, -3.0, 0.0)
            } else if p < 0.60 {
                (35_000.0, 452.0, 0.0, false, 35_000.0, 2.5, 0.0, 0.0)
            } else if p < 0.90 {
                let k = (p - 0.60) / 0.30;
                (35_000.0 - k * 33_000.0, 452.0 - k * 290.0, -1700.0, false, 35_000.0 - k * 33_000.0, -2.0, 1.5, 0.0)
            } else if p < 0.955 {
                let k = (p - 0.90) / 0.055;
                (2000.0 - k * 1994.0, 162.0 - k * 25.0, -710.0, false, 2000.0 - k * 1994.0, -4.5, 0.0, 0.0)
            } else if p < 0.962 {
                (0.0, 136.0, 0.0, true, 0.0, -1.0, 0.0, -2.3)
            } else {
                let k = (p - 0.962) / 0.038;
                (0.0, 136.0 * (1.0 - k).max(0.0), 0.0, true, 0.0, 0.0, 0.0, -2.3)
            };

            let progress = ((p - 0.05) / 0.90).clamp(0.0, 1.0);
            let lat = 41.8003 + (40.886 - 41.8003) * progress;
            let lon = 12.2389 + (14.2908 - 12.2389) * progress;
            let fuel = (8000.0 - 1150.0 * (p * cycle / 3600.0)).max(600.0);

            let telemetry = Telemetry {
                lat,
                lon,
                alt_ft: alt,
                agl_ft: agl,
                baro_alt_ft: alt,
                ias_kt: gs * 0.92,
                tas_kt: gs * 1.02,
                gs_kt: gs,
                vs_fpm: vs,
                hdg_mag: 128.0,
                hdg_true: 130.0,
                pitch_deg: pitch,
                bank_deg: bank,
                on_ground,
                wind_dir: 205.0,
                wind_kt: 9.0,
                oat_c: -46.0 + alt / 1000.0 * 2.0,
                fuel_kg: fuel,
                gear_down: p > 0.90,
                gear_pct: if p > 0.90 { 100.0 } else { 0.0 },
                flaps_pct: if p > 0.93 { 100.0 } else { 0.0 },
                park_brake: p < 0.04,
                autopilot: p > 0.7 && p < 0.93,
                engine_on: true,
                g_force: if on_ground { 1.0 } else { 1.0 },
                td_vs_fps: td_vs,
                td_bank_deg: 0.5,
                td_pitch_deg: -3.5,
                td_hdg_deg: 128.0,
                td_lat: lat.to_radians(),
                td_lon: lon.to_radians(),
                weight_kg: 62_000.0 - p * 5000.0,
                stall_warning: false,
                wp_dist_nm: 12.0,
                wp_ete_s: 240.0,
                wp_track: 128.0,
                squawk: "1000".to_string(),
                title: "Airbus A320neo — demo".to_string(),
                atc_model: "A320".to_string(),
                wp_next: "NEMBO".to_string(),
                atc_id: "I-DEMO".to_string(),
                atc_airline: "ITA Airways".to_string(),
                atc_flight: "610".to_string(),
            };
            let _ = tx.send(SimEvent::Telemetry(telemetry));
            std::thread::sleep(Duration::from_millis(120));
        }
    });
}

fn window_icon() -> egui::IconData {
    match image::load_from_memory(include_bytes!("../assets/icon_png/icon_256.png")) {
        Ok(image) => {
            let rgba = image.to_rgba8();
            let (width, height) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            }
        }
        Err(_) => egui::IconData {
            rgba: vec![0, 0, 0, 0],
            width: 1,
            height: 1,
        },
    }
}

/// Barlow, embedded so the app never depends on installed fonts.
fn install_fonts(ctx: &egui::Context) {
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "barlow".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Barlow-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "barlow-medium".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Barlow-Medium.ttf"
        ))),
    );
    fonts.font_data.insert(
        "barlow-bold".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Barlow-SemiBold.ttf"
        ))),
    );
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "barlow-medium".to_owned());
        family.insert(1, "barlow".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result {
    let demo = std::env::args().any(|arg| arg == "--demo");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 700.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(window_icon())
            .with_title("MSFS Flight Tracker"),
        ..Default::default()
    };
    eframe::run_native(
        "MSFS Flight Tracker",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new(demo)))
        }),
    )
}
