//! MSFS Flight Tracker installer.
//!
//! A small window that copies the app and SimConnect.dll into the user's local
//! programs folder, creates Desktop and Start Menu shortcuts and registers an
//! uninstall entry. The updater opens this window instead of replacing files
//! behind the pilot's back.
//!
//! Arguments: `/S` silent (scripts only), `/U` uninstall, `/U-now` internal
//! uninstall step, `/NOLAUNCH` do not start the app at the end.

#![windows_subsystem = "windows"]

use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use eframe::egui;

const APP_NAME: &str = "MSFS Flight Tracker";
const EXE_NAME: &str = "msfs-flight-tracker.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MB_YESNO: u32 = 0x0000_0004;
const MB_OK: u32 = 0x0000_0000;
const MB_ICONQUESTION: u32 = 0x0000_0020;
const MB_ICONINFORMATION: u32 = 0x0000_0040;
const IDYES: i32 = 6;

const APP: &[u8] = include_bytes!("../../dist/msfs-flight-tracker.exe");
const SIMCONNECT: &[u8] = include_bytes!("../../dist/SimConnect.dll");

const BG: egui::Color32 = egui::Color32::from_rgb(9, 7, 8);
const CARD: egui::Color32 = egui::Color32::from_rgb(20, 16, 18);
const BORDER: egui::Color32 = egui::Color32::from_rgb(56, 40, 44);
const TEXT: egui::Color32 = egui::Color32::from_rgb(245, 238, 238);
const MUTED: egui::Color32 = egui::Color32::from_rgb(174, 152, 155);
const DIM: egui::Color32 = egui::Color32::from_rgb(122, 100, 104);
const RED: egui::Color32 = egui::Color32::from_rgb(238, 66, 78);
const GREEN: egui::Color32 = egui::Color32::from_rgb(102, 214, 154);

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, flags: u32) -> i32;
}

/// The app version, read from the main crate so the two never drift.
fn version() -> String {
    include_str!("../../Cargo.toml")
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|rest| rest.split('"').next())
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|| "0.0.0".to_string())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn message(text: &str, caption: &str, flags: u32) -> i32 {
    let text = wide(text);
    let caption = wide(caption);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            flags,
        )
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|arg| arg.eq_ignore_ascii_case(flag));
    if has("/U") {
        begin_uninstall(has("/S"));
        return Ok(());
    }
    if has("/U-now") {
        uninstall_now(has("/S"));
        return Ok(());
    }
    if has("/S") {
        match install_with(&mut |_, _| {}, true, true) {
            Ok(dir) => {
                if !has("/NOLAUNCH") {
                    launch(&dir);
                }
            }
            Err(err) => {
                eprintln!("install: {err}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 470.0])
            .with_min_inner_size([560.0, 470.0])
            .with_max_inner_size([560.0, 470.0])
            .with_resizable(false)
            .with_icon(window_icon())
            .with_title(format!("{APP_NAME} Setup")),
        ..Default::default()
    };
    eframe::run_native(
        "MSFS Flight Tracker Setup",
        options,
        Box::new(|cc| {
            install_fonts(&cc.egui_ctx);
            apply_style(&cc.egui_ctx);
            Ok(Box::new(Setup::new()))
        }),
    )
}

fn window_icon() -> egui::IconData {
    match image::load_from_memory(include_bytes!("../../assets/icon_png/icon_256.png")) {
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

fn install_fonts(ctx: &egui::Context) {
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (
            "barlow",
            &include_bytes!("../../assets/fonts/Barlow-Regular.ttf")[..],
        ),
        (
            "barlow-medium",
            &include_bytes!("../../assets/fonts/Barlow-Medium.ttf")[..],
        ),
        (
            "barlow-bold",
            &include_bytes!("../../assets/fonts/Barlow-SemiBold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(egui::FontData::from_static(bytes)));
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "barlow-medium".to_owned());
        family.insert(1, "barlow".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn apply_style(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = BG;
    style.visuals.widgets.noninteractive.bg_fill = CARD;
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(28, 22, 25);
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(38, 28, 31);
    style.visuals.widgets.active.bg_fill = RED;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.text_styles = [
        (TextStyle::Body, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(10.5, FontFamily::Proportional)),
    ]
    .into();
    ctx.set_style(style);
}

enum Event {
    Step(f32, String),
    Finished(Result<PathBuf, String>),
}

enum Stage {
    Ready,
    Working,
    Done,
    Failed,
}

struct Setup {
    stage: Stage,
    progress: f32,
    step: String,
    error: String,
    desktop_shortcut: bool,
    start_menu_entry: bool,
    launch_when_done: bool,
    previous: Option<String>,
    installed_dir: Option<PathBuf>,
    rx: Option<Receiver<Event>>,
    logo: Option<egui::TextureHandle>,
    exit: bool,
}

impl Setup {
    fn new() -> Self {
        let previous = std::fs::read_to_string(install_dir().join("version.txt"))
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty() && *text != version());
        Self {
            stage: Stage::Ready,
            progress: 0.0,
            step: String::new(),
            error: String::new(),
            desktop_shortcut: true,
            start_menu_entry: true,
            launch_when_done: true,
            previous,
            installed_dir: None,
            rx: None,
            logo: None,
            exit: false,
        }
    }

    fn start(&mut self) {
        if matches!(self.stage, Stage::Working) {
            return;
        }
        self.stage = Stage::Working;
        self.progress = 0.0;
        self.step = "Preparing…".to_string();
        self.error.clear();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let desktop = self.desktop_shortcut;
        let menu = self.start_menu_entry;
        std::thread::spawn(move || {
            let sender = tx.clone();
            let result = install_with(
                &mut move |progress, step| {
                    let _ = sender.send(Event::Step(progress, step.to_string()));
                },
                desktop,
                menu,
            );
            let _ = tx.send(Event::Finished(result));
        });
    }

    fn pump(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::Step(progress, step) => {
                    self.progress = progress;
                    self.step = step;
                }
                Event::Finished(result) => {
                    match result {
                        Ok(dir) => {
                            self.installed_dir = Some(dir);
                            self.stage = Stage::Done;
                            self.progress = 1.0;
                        }
                        Err(err) => {
                            self.error = err;
                            self.stage = Stage::Failed;
                        }
                    }
                    self.rx = None;
                    break;
                }
            }
        }
    }
}

impl eframe::App for Setup {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();
        if ctx.input(|input| input.viewport().close_requested()) {
            if self.exit || matches!(self.stage, Stage::Ready | Stage::Done | Stage::Failed) {
                // let it close
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.logo.is_none() {
            if let Ok(image) = image::load_from_memory(include_bytes!("../../assets/icon_png/icon_128.png"))
            {
                let rgba = image.to_rgba8();
                let (width, height) = rgba.dimensions();
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    rgba.as_raw(),
                );
                self.logo = Some(ctx.load_texture("setup-logo", color, egui::TextureOptions::LINEAR));
            }
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(24, 20)),
            )
            .show(ctx, |ui| {
                self.header(ui);
                ui.add_space(16.0);
                match self.stage {
                    Stage::Ready => self.ready(ui),
                    Stage::Working => self.working(ui),
                    Stage::Done => self.done(ui),
                    Stage::Failed => self.failed(ui),
                }
                self.footer(ui);
            });

        if self.exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl Setup {
    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(
                    egui::Image::from_texture(logo)
                        .fit_to_exact_size(egui::vec2(48.0, 48.0))
                        .corner_radius(egui::CornerRadius::same(10)),
                );
            }
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(APP_NAME.to_uppercase()).size(17.0).color(TEXT).strong());
                ui.label(
                    egui::RichText::new(format!("SETUP · VERSION {}", version()))
                        .size(8.5)
                        .color(MUTED)
                        .strong(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(previous) = &self.previous {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(32, 20, 22))
                        .corner_radius(egui::CornerRadius::same(12))
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("UPDATE FROM {previous}"))
                                    .size(8.0)
                                    .color(egui::Color32::from_rgb(255, 160, 92))
                                    .strong(),
                            );
                        });
                }
            });
        });
    }

    fn ready(&mut self, ui: &mut egui::Ui) {
        let dir = install_dir();
        card(ui, |ui| {
            caption(ui, "INSTALLATION FOLDER");
            ui.label(
                egui::RichText::new(dir.display().to_string())
                    .size(11.5)
                    .family(egui::FontFamily::Monospace)
                    .color(TEXT),
            );
            ui.label(
                egui::RichText::new(
                    "Inside your user folder: no administrator rights, no services, no background installer.",
                )
                .size(10.0)
                .color(DIM),
            );
            ui.add_space(12.0);
            checkbox(ui, &mut self.desktop_shortcut, "Shortcut on the Desktop");
            checkbox(ui, &mut self.start_menu_entry, "Entry in the Start menu");
            checkbox(ui, &mut self.launch_when_done, "Start MSFS Flight Tracker when setup finishes");
            ui.add_space(14.0);
            if big_button(ui, "INSTALL", RED, TEXT).clicked() {
                self.start();
            }
        });
    }

    fn working(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            caption(ui, "INSTALLING");
            ui.label(
                egui::RichText::new(&self.step)
                    .size(13.5)
                    .color(TEXT)
                    .strong(),
            );
            ui.add_space(8.0);
            ui.add(
                egui::ProgressBar::new(self.progress)
                    .desired_height(7.0)
                    .corner_radius(egui::CornerRadius::same(4))
                    .fill(RED),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{:.0}%", self.progress * 100.0))
                        .size(10.5)
                        .color(MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("The running app is closed first")
                            .size(9.5)
                            .color(DIM),
                    );
                });
            });
        });
    }

    fn done(&mut self, ui: &mut egui::Ui) {
        let dir = self
            .installed_dir
            .clone()
            .unwrap_or_else(install_dir);
        card(ui, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(20, 38, 30))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("INSTALLED").size(8.5).color(GREEN).strong());
                });
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("{APP_NAME} {} is ready", version()))
                    .size(15.0)
                    .color(TEXT)
                    .strong(),
            );
            ui.label(
                egui::RichText::new(dir.display().to_string())
                    .size(10.5)
                    .family(egui::FontFamily::Monospace)
                    .color(MUTED),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Updates never install themselves: the app tells you when a new version exists and opens this setup.")
                    .size(10.0)
                    .color(DIM),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if big_button(ui, "FINISH", RED, TEXT).clicked() {
                    if self.launch_when_done {
                        launch(&dir);
                    }
                    self.exit = true;
                }
                ui.add_space(6.0);
                if small_button(ui, "Close").clicked() {
                    self.exit = true;
                }
            });
        });
    }

    fn failed(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            caption(ui, "SETUP FAILED");
            ui.label(
                egui::RichText::new(&self.error)
                    .size(12.0)
                    .color(egui::Color32::from_rgb(255, 120, 120)),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if big_button(ui, "TRY AGAIN", RED, TEXT).clicked() {
                    self.start();
                }
                ui.add_space(6.0);
                if small_button(ui, "Close").clicked() {
                    self.exit = true;
                }
            });
        });
    }

    fn footer(&self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.label(
                egui::RichText::new("Logbook and settings stay in %APPDATA%\\MSFSFlightTracker")
                    .size(9.0)
                    .color(DIM),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("github.com/Federicofnn/msfs-flight-tracker")
                    .size(9.0)
                    .color(DIM),
            );
        });
    }
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(egui::CornerRadius::same(16))
        .inner_margin(egui::Margin::symmetric(18, 16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(8.0)
            .color(RED)
            .strong(),
    );
    ui.add_space(4.0);
}

fn checkbox(ui: &mut egui::Ui, value: &mut bool, label: &str) {
    ui.horizontal(|ui| {
        if ui.checkbox(value, "").changed() {}
        ui.label(egui::RichText::new(label).size(11.5).color(MUTED));
    });
}

fn big_button(ui: &mut egui::Ui, text: &str, fill: egui::Color32, text_color: egui::Color32) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 40.0],
        egui::Button::new(
            egui::RichText::new(text)
                .size(12.5)
                .color(text_color)
                .strong(),
        )
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(10)),
    )
}

fn small_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_sized(
        [72.0, 40.0],
        egui::Button::new(egui::RichText::new(text).size(11.5).color(MUTED))
            .fill(egui::Color32::from_rgb(26, 21, 23))
            .corner_radius(egui::CornerRadius::same(10)),
    )
}

fn install_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Programs")
        .join(APP_NAME)
}

fn install_with(
    progress: &mut dyn FnMut(f32, &str),
    desktop: bool,
    menu: bool,
) -> Result<PathBuf, String> {
    let dir = install_dir();
    progress(0.03, "Preparing the folder…");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;

    progress(0.08, "Closing the running app…");
    stop_app();

    progress(0.18, "Copying MSFS Flight Tracker…");
    write_retry(&dir.join(EXE_NAME), APP)?;

    progress(0.42, "Copying SimConnect.dll…");
    write_retry(&dir.join("SimConnect.dll"), SIMCONNECT)?;

    progress(0.55, "Preparing the uninstaller…");
    write_retry(
        &dir.join("uninstall.exe"),
        &std::fs::read(std::env::current_exe().unwrap_or_default()).unwrap_or_default(),
    )?;
    std::fs::write(dir.join("version.txt"), version()).ok();

    if desktop || menu {
        progress(0.72, "Creating the shortcuts…");
        if let Err(err) = shortcuts(&dir, desktop, menu) {
            eprintln!("shortcuts: {err}");
        }
    }

    progress(0.9, "Registering the app…");
    register_uninstall(&dir);

    progress(1.0, "Done");
    Ok(dir)
}

fn launch(dir: &Path) {
    let _ = Command::new(dir.join(EXE_NAME))
        .current_dir(dir)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

fn stop_app() {
    let _ = Command::new("taskkill")
        .args(["/IM", EXE_NAME, "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    std::thread::sleep(Duration::from_millis(700));
}

fn write_retry(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..25 {
        match std::fs::write(path, bytes) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last = err.to_string();
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }
    Err(format!("Cannot write {}: {last}", path.display()))
}

fn shortcuts(dir: &Path, desktop: bool, menu: bool) -> Result<(), String> {
    let exe = dir.join(EXE_NAME);
    let script = r#"
$ErrorActionPreference = 'Stop'
$exe = $env:MSFSFT_EXE
$dir = $env:MSFSFT_DIR
$w = New-Object -ComObject WScript.Shell
$targets = @()
if ($env:MSFSFT_DESKTOP -eq '1') { $targets += [Environment]::GetFolderPath('Desktop') }
if ($env:MSFSFT_MENU -eq '1') { $targets += (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs') }
foreach ($target in $targets) {
  if (-not (Test-Path $target)) { continue }
  $lnk = Join-Path $target 'MSFS Flight Tracker.lnk'
  $s = $w.CreateShortcut($lnk)
  $s.TargetPath = $exe
  $s.WorkingDirectory = $dir
  $s.IconLocation = "$exe,0"
  $s.Description = 'MSFS Flight Tracker'
  $s.Save()
}
"#;
    let path = std::env::temp_dir().join("msfsft-shortcuts.ps1");
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            path.to_string_lossy().as_ref(),
        ])
        .env("MSFSFT_EXE", &exe)
        .env("MSFSFT_DIR", dir)
        .env("MSFSFT_DESKTOP", if desktop { "1" } else { "0" })
        .env("MSFSFT_MENU", if menu { "1" } else { "0" })
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "Windows could not create the shortcuts".to_string())
}

fn register_uninstall(dir: &Path) {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\MSFSFlightTracker";
    let uninstall_exe = dir.join("uninstall.exe");
    let icon = dir.join(EXE_NAME);
    let _ = Command::new("reg")
        .args(["add", key, "/v", "DisplayName", "/t", "REG_SZ", "/d", APP_NAME, "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    for (name, value) in [
        ("DisplayVersion", version()),
        ("Publisher", APP_NAME.to_string()),
        ("InstallLocation", dir.display().to_string()),
        ("DisplayIcon", format!("{},0", icon.display())),
        ("UninstallString", format!("\"{}\" /U", uninstall_exe.display())),
    ] {
        let _ = Command::new("reg")
            .args(["add", key, "/v", name, "/t", "REG_SZ", "/d", &value, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    for (name, value) in [("NoModify", "1"), ("NoRepair", "1")] {
        let _ = Command::new("reg")
            .args(["add", key, "/v", name, "/t", "REG_DWORD", "/d", value, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = Command::new("reg")
        .args(["add", key, "/v", "EstimatedSize", "/t", "REG_DWORD", "/d", "14000", "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

/// Copies the uninstaller out of the install folder, then hands over to it so
/// the folder can be deleted while this process exits.
fn begin_uninstall(silent: bool) {
    if !silent
        && message(
            &format!("Remove {APP_NAME} and its shortcuts?\n\nYour logbook is kept in %APPDATA%."),
            APP_NAME,
            MB_YESNO | MB_ICONQUESTION,
        ) != IDYES
    {
        return;
    }
    let temp = std::env::temp_dir().join("msfsft-uninstall.exe");
    let source = std::env::current_exe().unwrap_or_default();
    if std::fs::copy(&source, &temp).is_err() {
        uninstall_now(silent);
        return;
    }
    let mut child = Command::new(&temp);
    child.arg("/U-now");
    if silent {
        child.arg("/S");
    }
    let _ = child.creation_flags(CREATE_NO_WINDOW).spawn();
}

fn uninstall_now(silent: bool) {
    stop_app();
    remove_shortcuts();
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\MSFSFlightTracker";
    let _ = Command::new("reg")
        .args(["delete", key, "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    let _ = std::fs::remove_dir_all(install_dir());
    if !silent {
        message(
            &format!("{APP_NAME} has been removed. Your flight logbook is still in %APPDATA%."),
            APP_NAME,
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn remove_shortcuts() {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$targets = @([Environment]::GetFolderPath('Desktop'), (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'))
foreach ($target in $targets) {
  Remove-Item (Join-Path $target 'MSFS Flight Tracker.lnk') -Force
}
"#;
    let path = std::env::temp_dir().join("msfsft-clean.ps1");
    if std::fs::write(&path, script).is_ok() {
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                path.to_string_lossy().as_ref(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
}
