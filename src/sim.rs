//! Hand-written, zero-dependency SimConnect bindings.
//!
//! SimConnect.dll is loaded at runtime, so the app never fails to start when
//! the simulator (or the DLL) is missing: the worker thread retries and the UI
//! reports the state. Only a handful of the C API entry points are needed.

use std::ffi::{c_void, CString};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

type Handle = *mut c_void;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(file: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

type FnOpen =
    unsafe extern "system" fn(*mut Handle, *const u8, *mut c_void, u32, *mut c_void, u32) -> i32;
type FnClose = unsafe extern "system" fn(Handle) -> i32;
type FnAdd = unsafe extern "system" fn(Handle, u32, *const u8, *const u8, u32, f32, u32) -> i32;
type FnRequest =
    unsafe extern "system" fn(Handle, u32, u32, u32, u32, u32, u32, u32, u32) -> i32;
type FnNext = unsafe extern "system" fn(Handle, *mut *mut u8, *mut u32) -> i32;

struct Api {
    open: FnOpen,
    close: FnClose,
    add: FnAdd,
    request: FnRequest,
    next: FnNext,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    F64,
    I32,
    Str,
}

pub struct Field {
    pub name: &'static str,
    pub units: &'static str,
    pub ty: Ty,
}

/// Every datum is requested once, in this order; the decoder walks the same
/// table so offsets stay in sync automatically.
pub const FIELDS: &[Field] = &[
    Field { name: "PLANE LATITUDE", units: "degrees", ty: Ty::F64 },              // 0
    Field { name: "PLANE LONGITUDE", units: "degrees", ty: Ty::F64 },             // 1
    Field { name: "PLANE ALTITUDE", units: "feet", ty: Ty::F64 },                 // 2
    Field { name: "PLANE ALT ABOVE GROUND", units: "feet", ty: Ty::F64 },         // 3
    Field { name: "INDICATED ALTITUDE", units: "feet", ty: Ty::F64 },             // 4
    Field { name: "AIRSPEED INDICATED", units: "knots", ty: Ty::F64 },            // 5
    Field { name: "AIRSPEED TRUE", units: "knots", ty: Ty::F64 },                 // 6
    Field { name: "GROUND VELOCITY", units: "knots", ty: Ty::F64 },               // 7
    Field { name: "VERTICAL SPEED", units: "feet per minute", ty: Ty::F64 },      // 8
    Field { name: "PLANE HEADING DEGREES MAGNETIC", units: "degrees", ty: Ty::F64 }, // 9
    Field { name: "PLANE HEADING DEGREES TRUE", units: "degrees", ty: Ty::F64 },  // 10
    Field { name: "PLANE PITCH DEGREES", units: "degrees", ty: Ty::F64 },         // 11
    Field { name: "PLANE BANK DEGREES", units: "degrees", ty: Ty::F64 },          // 12
    Field { name: "SIM ON GROUND", units: "bool", ty: Ty::I32 },                  // 13
    Field { name: "AMBIENT WIND DIRECTION", units: "degrees", ty: Ty::F64 },      // 14
    Field { name: "AMBIENT WIND VELOCITY", units: "knots", ty: Ty::F64 },         // 15
    Field { name: "AMBIENT TEMPERATURE", units: "celsius", ty: Ty::F64 },         // 16
    Field { name: "FUEL TOTAL QUANTITY WEIGHT", units: "pounds", ty: Ty::F64 },   // 17
    Field { name: "GEAR HANDLE POSITION", units: "bool", ty: Ty::I32 },           // 18
    Field { name: "GEAR TOTAL PCT EXTENDED", units: "percent", ty: Ty::F64 },     // 19
    Field { name: "FLAPS HANDLE PERCENT", units: "percent", ty: Ty::F64 },        // 20
    Field { name: "BRAKE PARKING INDICATOR", units: "bool", ty: Ty::I32 },        // 21
    Field { name: "AUTOPILOT MASTER", units: "bool", ty: Ty::I32 },               // 22
    Field { name: "ENGINE COMBUSTION:1", units: "bool", ty: Ty::I32 },            // 23
    Field { name: "G FORCE", units: "GForce", ty: Ty::F64 },                      // 24
    Field { name: "PLANE TOUCHDOWN NORMAL VELOCITY", units: "feet per second", ty: Ty::F64 }, // 25
    Field { name: "PLANE TOUCHDOWN BANK DEGREES", units: "degrees", ty: Ty::F64 },    // 26
    Field { name: "PLANE TOUCHDOWN PITCH DEGREES", units: "degrees", ty: Ty::F64 },   // 27
    Field { name: "PLANE TOUCHDOWN HEADING DEGREES TRUE", units: "degrees", ty: Ty::F64 }, // 28
    Field { name: "PLANE TOUCHDOWN LATITUDE", units: "radians", ty: Ty::F64 },     // 29
    Field { name: "PLANE TOUCHDOWN LONGITUDE", units: "radians", ty: Ty::F64 },    // 30
    Field { name: "TOTAL WEIGHT", units: "pounds", ty: Ty::F64 },                 // 31
    Field { name: "STALL WARNING", units: "bool", ty: Ty::I32 },                  // 32
    Field { name: "GPS WP DISTANCE", units: "nautical miles", ty: Ty::F64 },      // 33
    Field { name: "GPS WP ETE", units: "seconds", ty: Ty::F64 },                  // 34
    Field { name: "GPS WP DESIRED TRACK", units: "degrees", ty: Ty::F64 },        // 35
    Field { name: "TRANSPONDER CODE:1", units: "Bco16", ty: Ty::I32 },             // 36
    Field { name: "TITLE", units: "", ty: Ty::Str },                              // 37
    Field { name: "ATC MODEL", units: "", ty: Ty::Str },                          // 38
    Field { name: "GPS WP NEXT ID", units: "", ty: Ty::Str },                     // 39
    Field { name: "ATC ID", units: "", ty: Ty::Str },                             // 40
    Field { name: "ATC AIRLINE", units: "", ty: Ty::Str },                        // 41
    Field { name: "ATC FLIGHT NUMBER", units: "", ty: Ty::Str },                  // 42
    // Second copy of the transponder code: some aircraft only answer it as text.
    Field { name: "TRANSPONDER CODE:1", units: "Bco16", ty: Ty::Str },             // 43
    // Transponder mode (0 off, 1 standby, 2 test, 3 on, 4 alt) and the code the
    // FlyByWire A32NX keeps in its own variable, which the stock one misses.
    Field { name: "TRANSPONDER STATE:1", units: "Enum", ty: Ty::F64 },             // 44
    Field { name: "L:A32NX_TRANSPONDER_CODE", units: "number", ty: Ty::F64 },      // 45
];

impl Ty {
    /// `SIMCONNECT_DATATYPE` value and width in bytes of the datum.
    fn wire(self) -> (u32, usize) {
        match self {
            Ty::F64 => (4, 8),
            Ty::I32 => (1, 4),
            Ty::Str => (9, 256), // SIMCONNECT_DATATYPE_STRING256
        }
    }
}

#[repr(usize)]
enum Idx {
    Lat = 0,
    Lon,
    Alt,
    Agl,
    BaroAlt,
    Ias,
    Tas,
    Gs,
    Vs,
    HdgMag,
    HdgTrue,
    Pitch,
    Bank,
    OnGround,
    WindDir,
    WindKt,
    Oat,
    FuelLb,
    GearHandle,
    GearPct,
    FlapsPct,
    ParkBrake,
    ApMaster,
    Eng1,
    GForce,
    TdVs,
    TdBank,
    TdPitch,
    TdHdg,
    TdLat,
    TdLon,
    TotalWeightLb,
    Stall,
    WpDist,
        WpEte,
    WpTrack,
    Squawk,
    Title,
    AtcModel,
    WpNext,
    AtcId,
    AtcAirline,
    AtcFlight,
    SquawkText,
    XpdrState,
    SquawkLvar,
}

/// The transponder code arrives as BCD (`0x1000` for squawk 1000) on most
/// aircraft and as a plain number on some. The text copy and the A32NX local
/// variable cover the aircraft that only publish it there.
fn squawk_code(raw: i64, text: &str, lvar: f64) -> String {
    if raw > 0 {
        let hex = format!("{raw:X}");
        if hex.len() <= 4 && hex.chars().all(|c| ('0'..='7').contains(&c)) {
            return format!("{hex:0>4}");
        }
        if raw <= 7_777 {
            return format!("{raw:04}");
        }
    }
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 4 {
        return digits;
    }
    if lvar.is_finite() && lvar > 0.0 && lvar <= 7_777.0 {
        return format!("{lvar:.0}");
    }
    String::new()
}

const DEF_ID: u32 = 1;
const REQ_ID: u32 = 1;

/// SIMCONNECT_RECV_SIMOBJECT_DATA header: size, version, id, request id,
/// object id, define id, flags, entry number, out of, define count.
const PACKET_HEAD: usize = 40;

/// What the simulator accepted from `FIELDS`, with the byte offset of every
/// datum inside the packet. When a variable is refused it is simply left out,
/// so one bad name never costs the whole telemetry stream.
struct Layout {
    slots: Vec<(usize, usize)>,
    bytes: usize,
}

struct Link {
    handle: Handle,
    layout: Layout,
    rejected: Vec<&'static str>,
}

#[derive(Clone, Debug)]
pub struct Telemetry {
    pub lat: f64,
    pub lon: f64,
    pub alt_ft: f64,
    pub agl_ft: f64,
    pub baro_alt_ft: f64,
    pub ias_kt: f64,
    pub tas_kt: f64,
    pub gs_kt: f64,
    pub vs_fpm: f64,
    pub hdg_mag: f64,
    pub hdg_true: f64,
    pub pitch_deg: f64,
    pub bank_deg: f64,
    pub on_ground: bool,
    pub wind_dir: f64,
    pub wind_kt: f64,
    pub oat_c: f64,
    pub fuel_kg: f64,
    pub gear_down: bool,
    pub gear_pct: f64,
    pub flaps_pct: f64,
    pub park_brake: bool,
    pub autopilot: bool,
    pub engine_on: bool,
    pub g_force: f64,
    pub td_vs_fps: f64,
    pub td_bank_deg: f64,
    pub td_pitch_deg: f64,
    pub td_hdg_deg: f64,
    pub td_lat: f64,
    pub td_lon: f64,
    pub weight_kg: f64,
    pub stall_warning: bool,
    pub wp_dist_nm: f64,
    pub wp_ete_s: f64,
    pub wp_track: f64,
    pub squawk: String,
    /// 0 off, 1 standby, 2 test, 3 on, 4 alt; NaN when the aircraft is silent.
    pub xpdr_state: f64,
    pub title: String,
    pub atc_model: String,
    pub wp_next: String,
    pub atc_id: String,
    pub atc_airline: String,
    pub atc_flight: String,
}

const LB_TO_KG: f64 = 0.453_592_37;

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            lat: f64::NAN,
            lon: f64::NAN,
            alt_ft: f64::NAN,
            agl_ft: f64::NAN,
            baro_alt_ft: f64::NAN,
            ias_kt: f64::NAN,
            tas_kt: f64::NAN,
            gs_kt: f64::NAN,
            vs_fpm: f64::NAN,
            hdg_mag: f64::NAN,
            hdg_true: f64::NAN,
            pitch_deg: f64::NAN,
            bank_deg: f64::NAN,
            on_ground: true,
            wind_dir: f64::NAN,
            wind_kt: f64::NAN,
            oat_c: f64::NAN,
            fuel_kg: f64::NAN,
            gear_down: false,
            gear_pct: f64::NAN,
            flaps_pct: f64::NAN,
            park_brake: false,
            autopilot: false,
            engine_on: false,
            g_force: f64::NAN,
            td_vs_fps: f64::NAN,
            td_bank_deg: f64::NAN,
            td_pitch_deg: f64::NAN,
            td_hdg_deg: f64::NAN,
            td_lat: f64::NAN,
            td_lon: f64::NAN,
            weight_kg: f64::NAN,
            stall_warning: false,
            wp_dist_nm: f64::NAN,
            wp_ete_s: f64::NAN,
            wp_track: f64::NAN,
            squawk: String::new(),
            xpdr_state: f64::NAN,
            title: String::new(),
            atc_model: String::new(),
            wp_next: String::new(),
            atc_id: String::new(),
            atc_airline: String::new(),
            atc_flight: String::new(),
        }
    }
}

impl Telemetry {
    pub fn position_valid(&self) -> bool {
        self.lat.is_finite() && self.lon.is_finite() && self.lat != 0.0 && self.lon != 0.0
    }
}

pub enum SimEvent {
    Telemetry(Telemetry),
    Status(Status),
    Log(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Starting,
    NoDll,
    WaitingSim,
    Connected,
}

pub struct Status {
    pub state: LinkState,
    pub detail: String,
}

pub fn spawn(tx: Sender<SimEvent>) {
    let _ = std::thread::Builder::new()
        .name("simconnect".into())
        .spawn(move || worker(tx));
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn sym(module: *mut c_void, name: &str) -> Option<*mut c_void> {
    let c = CString::new(name).ok()?;
    let p = GetProcAddress(module, c.as_ptr() as *const u8);
    if p.is_null() {
        None
    } else {
        Some(p)
    }
}

fn load_api() -> Result<Api, String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("SimConnect.dll").to_string_lossy().into_owned());
            candidates.push(
                dir.join("resources")
                    .join("SimConnect.dll")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    candidates.push("SimConnect.dll".to_string());

    let mut tried = Vec::new();
    for path in &candidates {
        let w = wide(path);
        let module = unsafe { LoadLibraryW(w.as_ptr()) };
        if module.is_null() {
            tried.push(path.clone());
            continue;
        }
        unsafe {
            let open = sym(module, "SimConnect_Open");
            let close = sym(module, "SimConnect_Close");
            let add = sym(module, "SimConnect_AddToDataDefinition");
            let request = sym(module, "SimConnect_RequestDataOnSimObject");
            let next = sym(module, "SimConnect_GetNextDispatch");
            if let (Some(o), Some(c), Some(a), Some(r), Some(n)) = (open, close, add, request, next) {
                return Ok(Api {
                    open: std::mem::transmute::<*mut c_void, FnOpen>(o),
                    close: std::mem::transmute::<*mut c_void, FnClose>(c),
                    add: std::mem::transmute::<*mut c_void, FnAdd>(a),
                    request: std::mem::transmute::<*mut c_void, FnRequest>(r),
                    next: std::mem::transmute::<*mut c_void, FnNext>(n),
                });
            }
        }
        tried.push(format!("{path} (missing exports)"));
    }
    Err(format!(
        "SimConnect.dll not found. Place it next to the .exe. Tried: {}",
        tried.join(", ")
    ))
}

fn worker(tx: Sender<SimEvent>) {
    loop {
        let api = match load_api() {
            Ok(api) => api,
            Err(err) => {
                let _ = tx.send(SimEvent::Status(Status {
                    state: LinkState::NoDll,
                    detail: err,
                }));
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
        };

        let _ = tx.send(SimEvent::Status(Status {
            state: LinkState::WaitingSim,
            detail: "Looking for Microsoft Flight Simulator…".to_string(),
        }));

        loop {
            match unsafe { connect(&api) } {
                Ok(link) => {
                    let mut detail = format!(
                        "SimConnect link established · {} variables ({})",
                        link.layout.slots.len(),
                        link.layout.bytes
                    );
                    if !link.rejected.is_empty() {
                        detail.push_str(&format!(" · {} refused", link.rejected.len()));
                    }
                    let _ = tx.send(SimEvent::Status(Status {
                        state: LinkState::Connected,
                        detail,
                    }));
                    if !link.rejected.is_empty() {
                        let _ = tx.send(SimEvent::Log(format!(
                            "The simulator refused {} variable(s): {}",
                            link.rejected.len(),
                            link.rejected.join(", ")
                        )));
                    }
                    unsafe { read_loop(&api, &link, &tx) };
                    unsafe { (api.close)(link.handle) };
                    break;
                }
                Err(err) => {
                    let _ = tx.send(SimEvent::Status(Status {
                        state: LinkState::WaitingSim,
                        detail: err,
                    }));
                    std::thread::sleep(Duration::from_millis(2500));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

unsafe fn connect(api: &Api) -> Result<Link, String> {
    let mut handle: Handle = std::ptr::null_mut();
    let app_name = b"MSFS Flight Tracker\0";
    let hr = (api.open)(
        &mut handle,
        app_name.as_ptr(),
        std::ptr::null_mut(),
        0,
        std::ptr::null_mut(),
        0,
    );
    if hr != 0 || handle.is_null() {
        return Err(format!(
            "Microsoft Flight Simulator is not running (SimConnect_Open 0x{:08X})",
            hr as u32
        ));
    }

    let mut slots: Vec<(usize, usize)> = Vec::with_capacity(FIELDS.len());
    let mut rejected: Vec<&'static str> = Vec::new();
    let mut bytes = 0usize;
    for (index, field) in FIELDS.iter().enumerate() {
        let name = CString::new(field.name).unwrap_or_default();
        let units = CString::new(field.units).unwrap_or_default();
        let (ty, width) = field.ty.wire();
        let hr = (api.add)(
            handle,
            DEF_ID,
            name.as_ptr() as *const u8,
            units.as_ptr() as *const u8,
            ty,
            0.0,
            u32::MAX,
        );
        if hr != 0 {
            // Simulators differ in the variables they still provide: skip the
            // odd one out instead of losing every reading.
            rejected.push(field.name);
            continue;
        }
        slots.push((index, PACKET_HEAD + bytes));
        bytes += width;
    }

    if slots.len() < FIELDS.len() / 2 {
        (api.close)(handle);
        return Err(format!(
            "The simulator refused too many variables ({} of {}): {}",
            rejected.len(),
            FIELDS.len(),
            rejected.join(", ")
        ));
    }

    // SIMCONNECT_PERIOD_SIM_FRAME = 3
    let hr = (api.request)(handle, REQ_ID, DEF_ID, 0, 3, 0, 0, 0, 0);
    if hr != 0 {
        (api.close)(handle);
        return Err(format!(
            "SimConnect_RequestDataOnSimObject failed (0x{:08X})",
            hr as u32
        ));
    }
    Ok(Link {
        handle,
        layout: Layout { slots, bytes },
        rejected,
    })
}

#[inline]
unsafe fn rd_u32(base: *const u8, off: usize) -> u32 {
    std::ptr::read_unaligned(base.add(off) as *const u32)
}

#[inline]
unsafe fn rd_f64(base: *const u8, off: usize) -> f64 {
    std::ptr::read_unaligned(base.add(off) as *const f64)
}

unsafe fn decode(base: *const u8, size: u32, layout: &Layout) -> Option<Telemetry> {
    let size = size as usize;
    if size < PACKET_HEAD {
        return None;
    }
    let mut numbers: Vec<f64> = vec![f64::NAN; FIELDS.len()];
    let mut strings: Vec<String> = vec![String::new(); FIELDS.len()];

    for &(index, offset) in &layout.slots {
        let field = &FIELDS[index];
        match field.ty {
            Ty::F64 => {
                if offset + 8 <= size {
                    numbers[index] = rd_f64(base, offset);
                }
            }
            Ty::I32 => {
                if offset + 4 <= size {
                    numbers[index] = rd_u32(base, offset) as i32 as f64;
                }
            }
            Ty::Str => {
                const LEN: usize = 256;
                if offset + LEN <= size {
                    let bytes = std::slice::from_raw_parts(base.add(offset), LEN);
                    let end = bytes.iter().position(|b| *b == 0).unwrap_or(LEN);
                    strings[index] = String::from_utf8_lossy(&bytes[..end]).trim().to_string();
                }
            }
        }
    }

    let n = |i: Idx| numbers[i as usize];
    let s = |i: Idx| strings[i as usize].clone();

    let rad_to_deg = |v: f64| {
        if v.is_finite() && v.abs() <= 3.2 {
            v.to_degrees()
        } else {
            v
        }
    };

    Some(Telemetry {
        lat: n(Idx::Lat),
        lon: n(Idx::Lon),
        alt_ft: n(Idx::Alt),
        agl_ft: n(Idx::Agl),
        baro_alt_ft: n(Idx::BaroAlt),
        ias_kt: n(Idx::Ias),
        tas_kt: n(Idx::Tas),
        gs_kt: n(Idx::Gs),
        vs_fpm: n(Idx::Vs),
        hdg_mag: n(Idx::HdgMag),
        hdg_true: n(Idx::HdgTrue),
        pitch_deg: n(Idx::Pitch),
        bank_deg: n(Idx::Bank),
        on_ground: n(Idx::OnGround) != 0.0,
        wind_dir: n(Idx::WindDir),
        wind_kt: n(Idx::WindKt),
        oat_c: n(Idx::Oat),
        fuel_kg: n(Idx::FuelLb) * LB_TO_KG,
        gear_down: n(Idx::GearHandle) != 0.0,
        gear_pct: n(Idx::GearPct),
        flaps_pct: n(Idx::FlapsPct),
        park_brake: n(Idx::ParkBrake) != 0.0,
        autopilot: n(Idx::ApMaster) != 0.0,
        engine_on: n(Idx::Eng1) != 0.0,
        g_force: n(Idx::GForce),
        td_vs_fps: n(Idx::TdVs),
        td_bank_deg: n(Idx::TdBank),
        td_pitch_deg: n(Idx::TdPitch),
        td_hdg_deg: n(Idx::TdHdg),
        td_lat: rad_to_deg(n(Idx::TdLat)),
        td_lon: rad_to_deg(n(Idx::TdLon)),
        weight_kg: n(Idx::TotalWeightLb) * LB_TO_KG,
        stall_warning: n(Idx::Stall) != 0.0,
        wp_dist_nm: n(Idx::WpDist),
        wp_ete_s: n(Idx::WpEte),
        wp_track: n(Idx::WpTrack),
        squawk: squawk_code(n(Idx::Squawk) as i64, &s(Idx::SquawkText), n(Idx::SquawkLvar)),
        xpdr_state: n(Idx::XpdrState),
        title: s(Idx::Title),
        atc_model: s(Idx::AtcModel),
        wp_next: s(Idx::WpNext),
        atc_id: s(Idx::AtcId),
        atc_airline: s(Idx::AtcAirline),
        atc_flight: s(Idx::AtcFlight),
    })
}

unsafe fn read_loop(api: &Api, link: &Link, tx: &Sender<SimEvent>) {
    let handle = link.handle;
    let mut last_packet = Instant::now();
    let mut last_send = Instant::now() - Duration::from_secs(1);
    let mut reasked = false;

    loop {
        let mut data: *mut u8 = std::ptr::null_mut();
        let mut size: u32 = 0;
        let hr = (api.next)(handle, &mut data, &mut size);
        if hr == 0 && !data.is_null() {
            last_packet = Instant::now();
            let id = rd_u32(data, 8);
            match id {
                8 => {
                    // SIMCONNECT_RECV_ID_SIMOBJECT_DATA
                    if last_send.elapsed() >= Duration::from_millis(60) {
                        if let Some(t) = decode(data, size, &link.layout) {
                            last_send = Instant::now();
                            if tx.send(SimEvent::Telemetry(t)).is_err() {
                                return;
                            }
                        }
                    }
                }
                1 => {
                    // SIMCONNECT_RECV_ID_EXCEPTION
                    let code = rd_u32(data, 12);
                    let index = rd_u32(data, 20);
                    let _ = tx.send(SimEvent::Log(format!(
                        "SimConnect exception {code} (datum #{index})"
                    )));
                }
                3 => return, // SIMCONNECT_RECV_ID_QUIT
                _ => {}
            }
        } else {
            std::thread::sleep(Duration::from_millis(5));
            let silent = last_packet.elapsed();
            if !reasked && silent > Duration::from_secs(4) {
                // A definition requested while the simulator was still at its
                // main menu can go unanswered; ask once more before dropping
                // the link and starting over.
                reasked = true;
                let _ = (api.request)(handle, REQ_ID, DEF_ID, 0, 3, 0, 0, 0, 0);
            }
            if silent > Duration::from_secs(20) {
                return; // link went silent: reconnect
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_layout() -> Layout {
        let mut slots = Vec::new();
        let mut bytes = 0usize;
        for (index, field) in FIELDS.iter().enumerate() {
            slots.push((index, PACKET_HEAD + bytes));
            bytes += field.ty.wire().1;
        }
        Layout { slots, bytes }
    }

    /// A packet laid out the way SimConnect answers our definition.
    fn packet(layout: &Layout) -> Vec<u8> {
        let mut buf = vec![0u8; PACKET_HEAD + layout.bytes];
        let total = buf.len() as u32;
        buf[0..4].copy_from_slice(&total.to_le_bytes());
        buf[8..12].copy_from_slice(&8u32.to_le_bytes());
        for (index, field) in FIELDS.iter().enumerate() {
            let Some(slot) = layout.slots.iter().find(|(i, _)| *i == index) else {
                continue;
            };
            let offset = slot.1;
            match field.ty {
                Ty::F64 => {
                    let value = index as f64 + 0.5;
                    buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
                }
                Ty::I32 => buf[offset..offset + 4].copy_from_slice(&1i32.to_le_bytes()),
                Ty::Str => buf[offset..offset + 4].copy_from_slice(b"TEST"),
            }
        }
        buf
    }

    #[test]
    fn decodes_every_field() {
        let layout = full_layout();
        let buf = packet(&layout);
        let tele = unsafe { decode(buf.as_ptr(), buf.len() as u32, &layout) }.expect("packet");
        assert!((tele.lat - 0.5).abs() < 1e-9);
        assert!((tele.bank_deg - 12.5).abs() < 1e-9);
        assert!((tele.wp_track - 35.5).abs() < 1e-9);
        assert!(tele.on_ground && tele.park_brake && tele.autopilot);
        assert_eq!(tele.title, "TEST");
        assert_eq!(tele.atc_flight, "TEST");
        assert!(!tele.squawk.is_empty());
    }

    /// Not part of the normal run: loads the real DLL and prints exactly what
    /// the tracker would show, so a live simulator can be checked from a shell.
    #[test]
    #[ignore]
    fn reports_the_link_state() {
        match load_api() {
            Ok(api) => match unsafe { connect(&api) } {
                Ok(link) => {
                    println!(
                        "connected: {} variables, {} bytes, refused: {:?}",
                        link.layout.slots.len(),
                        link.layout.bytes,
                        link.rejected
                    );
                    unsafe { (api.close)(link.handle) };
                }
                Err(err) => println!("not connected: {err}"),
            },
            Err(err) => println!("dll: {err}"),
        }
    }

    #[test]
    fn squawk_reads_bcd_plain_numbers_and_local_variables() {
        assert_eq!(squawk_code(0x1000, "", f64::NAN), "1000");
        assert_eq!(squawk_code(0x1200, "", f64::NAN), "1200");
        assert_eq!(squawk_code(4321, "", f64::NAN), "4321");
        assert_eq!(squawk_code(1200, "", f64::NAN), "1200");
        assert_eq!(squawk_code(0, "1000", f64::NAN), "1000");
        assert_eq!(squawk_code(0, "SPD 7700", f64::NAN), "7700");
        assert_eq!(squawk_code(0, "", 4521.0), "4521");
        assert_eq!(squawk_code(0, "", 0.0), "");
        assert_eq!(squawk_code(0, "", f64::NAN), "");
    }

    #[test]
    fn a_refused_variable_keeps_the_rest_aligned() {
        let mut slots = Vec::new();
        let mut bytes = 0usize;
        for (index, field) in FIELDS.iter().enumerate() {
            if index == Idx::HdgTrue as usize {
                continue;
            }
            slots.push((index, PACKET_HEAD + bytes));
            bytes += field.ty.wire().1;
        }
        let layout = Layout { slots, bytes };
        let buf = packet(&layout);
        let tele = unsafe { decode(buf.as_ptr(), buf.len() as u32, &layout) }.expect("packet");
        assert!(tele.hdg_true.is_nan());
        assert!((tele.bank_deg - 12.5).abs() < 1e-9);
        assert_eq!(tele.title, "TEST");
    }
}
