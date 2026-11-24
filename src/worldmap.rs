//! Offline vector world map (Natural Earth 110m) with pan, zoom and overlays.
//!
//! The land fill is rasterised once per view change into a texture; coastlines,
//! borders, route, trail and the aircraft are drawn as vectors on top, so the
//! map stays sharp at any zoom without downloading map tiles.

use crate::plan::FlightPlan;
use crate::theme::*;
use crate::util::haversine_nm;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke,
    Ui,
};

const OCEAN: Color32 = Color32::from_rgb(10, 8, 10);
const LAND: Color32 = Color32::from_rgb(34, 27, 29);
const COAST: Color32 = Color32::from_rgb(82, 62, 66);
const GRID: Color32 = Color32::from_rgb(32, 24, 27);
const PLACE: Color32 = Color32::from_rgb(116, 98, 101);
const MAX_LAT: f64 = 85.05;

pub struct Ring {
    pub points: Vec<[f32; 2]>,
    /// min_lon, min_lat, max_lon, max_lat
    pub bounds: [f32; 4],
}

impl Ring {
    fn touches(&self, lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64) -> bool {
        let [min_lon, min_lat, max_lon, max_lat] = self.bounds;
        if (max_lat as f64) < lat_min || (min_lat as f64) > lat_max {
            return false;
        }
        if lon_max < lon_min {
            return true;
        }
        (max_lon as f64) >= lon_min && (min_lon as f64) <= lon_max
    }
}

pub struct World {
    pub land: Vec<Ring>,
    pub borders: Vec<Ring>,
    pub places: Places,
    pub airports: Airports,
}

/// A country label taken from Natural Earth (name plus its label point).
pub struct Place {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub min_zoom: f64,
    pub max_zoom: f64,
}

pub struct Places {
    pub list: Vec<Place>,
}

impl Places {
    fn embedded() -> Self {
        let bytes: &[u8] = include_bytes!("../assets/places.bin");
        let mut list = Vec::new();
        if bytes.len() > 8 {
            let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4])) as usize;
            let mut offset = 8usize;
            for _ in 0..count {
                if offset + 11 > bytes.len() {
                    break;
                }
                let lat = f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap_or([0; 4])) as f64;
                let lon = f32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap_or([0; 4])) as f64;
                let min_zoom = bytes[offset + 8] as f64;
                let max_zoom = bytes[offset + 9] as f64;
                let len = bytes[offset + 10] as usize;
                let end = offset + 11 + len;
                if end > bytes.len() {
                    break;
                }
                list.push(Place {
                    name: String::from_utf8_lossy(&bytes[offset + 11..end]).to_string(),
                    lat,
                    lon,
                    min_zoom,
                    max_zoom,
                });
                offset = end;
            }
        }
        Places { list }
    }
}

/// A scheduled-service airport with scheduled service (large and medium).
pub struct Airport {
    pub ident: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// 0 = large airport, 1 = medium airport.
    pub class: u8,
}

pub struct Airports {
    pub list: Vec<Airport>,
}

impl Airports {
    fn embedded() -> Self {
        let bytes: &[u8] = include_bytes!("../assets/airports.bin");
        let mut list = Vec::new();
        if bytes.len() > 8 {
            let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4])) as usize;
            let mut offset = 8usize;
            for _ in 0..count {
                if offset + 14 > bytes.len() {
                    break;
                }
                let lat = f32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap_or([0; 4])) as f64;
                let lon = f32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap_or([0; 4])) as f64;
                let class = bytes[offset + 12];
                let len = bytes[offset + 13] as usize;
                let end = offset + 14 + len;
                if end > bytes.len() {
                    break;
                }
                list.push(Airport {
                    ident: String::from_utf8_lossy(&bytes[offset..offset + 4]).trim().to_string(),
                    lat,
                    lon,
                    class,
                    name: String::from_utf8_lossy(&bytes[offset + 14..end]).to_string(),
                });
                offset = end;
            }
        }
        Airports { list }
    }

    /// ICAO matches first, then airports whose name contains the query.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&Airport> {
        let query = query.trim().to_ascii_uppercase();
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut exact = Vec::new();
        let mut prefix = Vec::new();
        let mut contains = Vec::new();
        for airport in &self.list {
            if airport.ident == query {
                exact.push(airport);
            } else if airport.ident.starts_with(&query) {
                prefix.push(airport);
            } else if airport.name.to_ascii_uppercase().contains(&query) {
                contains.push(airport);
            }
        }
        exact
            .into_iter()
            .chain(prefix)
            .chain(contains)
            .take(limit)
            .collect()
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn u32(&mut self) -> u32 {
        let value = u32::from_le_bytes(
            self.bytes[self.offset..self.offset + 4]
                .try_into()
                .unwrap_or([0, 0, 0, 0]),
        );
        self.offset += 4;
        value
    }

    fn f32(&mut self) -> f32 {
        let value = f32::from_le_bytes(
            self.bytes[self.offset..self.offset + 4]
                .try_into()
                .unwrap_or([0, 0, 0, 0]),
        );
        self.offset += 4;
        value
    }

    fn lines(&mut self) -> Vec<Ring> {
        let count = self.u32() as usize;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let points = self.u32() as usize;
            let mut line = Vec::with_capacity(points);
            for _ in 0..points {
                let lon = self.f32();
                let lat = self.f32();
                line.push([lon, lat]);
            }
            let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for point in &line {
                bounds[0] = bounds[0].min(point[0]);
                bounds[1] = bounds[1].min(point[1]);
                bounds[2] = bounds[2].max(point[0]);
                bounds[3] = bounds[3].max(point[1]);
            }
            out.push(Ring { points: line, bounds });
        }
        out
    }
}

impl World {
    #[allow(dead_code)]
    pub fn embedded() -> Self {
        let bytes: &[u8] = include_bytes!("../assets/world.bin");
        let mut cursor = Cursor { bytes, offset: 4 };
        let land = cursor.lines();
        let borders = cursor.lines();
        World {
            land,
            borders,
            places: Places::embedded(),
            airports: Airports::embedded(),
        }
    }
}

fn world_size(zoom: f64) -> f64 {
    256.0 * 2f64.powf(zoom)
}

pub fn project(lat: f64, lon: f64, zoom: f64) -> (f64, f64) {
    let size = world_size(zoom);
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    let x = (lon + 180.0) / 360.0 * size;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * size;
    (x, y)
}

pub fn unproject(x: f64, y: f64, zoom: f64) -> (f64, f64) {
    let size = world_size(zoom);
    let lon = x / size * 360.0 - 180.0;
    let n = std::f64::consts::PI * (1.0 - 2.0 * y / size);
    let lat = n.sinh().atan().to_degrees();
    (lat, lon)
}

#[derive(Clone, Copy, PartialEq)]
struct CacheKey {
    lat: i64,
    lon: i64,
    zoom: i64,
    width: u32,
    height: u32,
}

struct Cache {
    key: CacheKey,
    texture: egui::TextureHandle,
}

pub struct MapState {
    pub center_lat: f64,
    pub center_lon: f64,
    pub zoom: f64,
    pub follow: bool,
    pub show_route: bool,
    pub show_trail: bool,
    pub show_labels: bool,
    pub show_grid: bool,
    pub show_airports: bool,
    pub show_places: bool,
    pub show_traffic: bool,
    cache: Option<Cache>,
}

impl Default for MapState {
    fn default() -> Self {
        Self {
            center_lat: 44.5,
            center_lon: 10.5,
            zoom: 4.4,
            follow: true,
            show_route: true,
            show_trail: true,
            show_labels: true,
            show_grid: false,
            show_airports: true,
            show_places: true,
            show_traffic: true,
            cache: None,
        }
    }
}

pub struct MapInput<'a> {
    pub aircraft: Option<(f64, f64, f64)>,
    pub plan: Option<&'a FlightPlan>,
    pub trail: &'a [(f64, f64)],
    /// Airports that match the search box, drawn with a highlight ring.
    pub matches: &'a [String],
    pub query: &'a str,
    /// Live network traffic to paint over the terrain.
    pub blips: &'a [crate::traffic::Blip],
}

impl MapState {
    pub fn zoom_by(&mut self, delta: f64) {
        self.zoom = (self.zoom + delta).clamp(1.2, 11.0);
    }

    pub fn center_on(&mut self, lat: f64, lon: f64) {
        self.center_lat = lat;
        self.center_lon = lon;
    }

    pub fn screen_pos(&self, rect: Rect, lat: f64, lon: f64) -> Pos2 {
        self.screen_of(rect, lat, lon)
    }

    fn screen_of(&self, rect: Rect, lat: f64, lon: f64) -> Pos2 {
        let (cx, cy) = project(self.center_lat, self.center_lon, self.zoom);
        let (x, y) = project(lat, lon, self.zoom);
        pos2(
            rect.center().x + (x - cx) as f32,
            rect.center().y + (y - cy) as f32,
        )
    }

    pub fn show(&mut self, ui: &mut Ui, rect: Rect, world: &World, input: &MapInput) -> Response {
        let response = ui.allocate_rect(rect, Sense::click_and_drag());

        if response.dragged() {
            let delta = response.drag_delta();
            let (cx, cy) = project(self.center_lat, self.center_lon, self.zoom);
            let (lat, lon) = unproject(
                cx - delta.x as f64,
                cy - delta.y as f64,
                self.zoom,
            );
            self.center_lat = lat;
            self.center_lon = lon;
            self.follow = false;
        }

        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y) as f64;
            if scroll.abs() > 0.5 {
                let pointer = ui
                    .input(|i| i.pointer.hover_pos())
                    .unwrap_or_else(|| rect.center());
                let before = self.geo_at(rect, pointer);
                self.zoom = (self.zoom + scroll * 0.0022).clamp(1.2, 11.0);
                let after = self.geo_at(rect, pointer);
                self.center_lat += before.0 - after.0;
                self.center_lon += before.1 - after.1;
                self.follow = false;
            }
        }

        if response.double_clicked() {
            self.follow = true;
        }

        if self.follow {
            if let Some((lat, lon, _)) = input.aircraft {
                self.center_lat = lat;
                self.center_lon = lon;
            }
        }

        self.center_lat = self.center_lat.clamp(-80.0, 80.0);
        if self.center_lon > 180.0 {
            self.center_lon -= 360.0;
        }
        if self.center_lon < -180.0 {
            self.center_lon += 360.0;
        }

        self.ensure_terrain(ui.ctx(), world, rect);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, OCEAN);
        if let Some(cache) = &self.cache {
            painter.image(
                cache.texture.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        if self.show_grid {
            self.draw_graticule(&painter, rect);
        }

        self.draw_borders(&painter, rect, world);
        self.draw_coastlines(&painter, rect, world);

        if self.show_places {
            self.draw_places(&painter, rect, world);
        }
        if self.show_airports {
            self.draw_airports(&painter, rect, world, input.matches, input.query);
        }

        if self.show_traffic && !input.blips.is_empty() {
            self.draw_traffic(&painter, rect, input.blips);
        }

        if self.show_route {
            if let Some(plan) = input.plan {
                self.draw_route(&painter, rect, plan);
            }
        }

        if self.show_trail && input.trail.len() > 1 {
            let points: Vec<Pos2> = input
                .trail
                .iter()
                .filter_map(|(lat, lon)| {
                    let p = self.screen_of(rect, *lat, *lon);
                    rect.expand(200.0).contains(p).then_some(p)
                })
                .collect();
            if points.len() > 1 {
                painter.add(egui::Shape::line(points, Stroke::new(2.0_f32, CYAN)));
            }
        }

        if let Some((lat, lon, heading)) = input.aircraft {
            self.draw_aircraft(&painter, rect, lat, lon, heading);
        }

        self.draw_scale(&painter, rect);

        response
    }

    /// Live pilots from VATSIM and IVAO, as small heading triangles.
    fn draw_traffic(&self, painter: &egui::Painter, rect: Rect, blips: &[crate::traffic::Blip]) {
        let vatsim = Color32::from_rgb(255, 178, 92);
        let ivao = Color32::from_rgb(118, 198, 255);
        let mut drawn = 0usize;
        for blip in blips {
            if drawn >= 3000 {
                break;
            }
            let p = self.screen_of(rect, blip.lat as f64, blip.lon as f64);
            if !rect.contains(p) {
                continue;
            }
            drawn += 1;
            let color = if blip.network == crate::traffic::IVAO {
                ivao
            } else {
                vatsim
            };
            let color = if blip.on_ground {
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 120)
            } else {
                color
            };
            let angle = (blip.hdg as f32).to_radians();
            let forward = egui::Vec2::angled(angle);
            let side = egui::vec2(-forward.y, forward.x);
            let tip = p + forward * 5.0;
            let left = p - forward * 3.4 + side * 3.0;
            let right = p - forward * 3.4 - side * 3.0;
            painter.circle_filled(p, 6.2, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 22));
            painter.add(egui::Shape::convex_polygon(
                vec![tip, left, right],
                color,
                Stroke::new(0.7_f32, Color32::from_rgb(28, 18, 20)),
            ));
        }
    }

    fn geo_bounds(&self, rect: Rect) -> (f64, f64, f64, f64) {
        let (lat_top, lon_left) = self.geo_at(rect, rect.left_top());
        let (lat_bottom, lon_right) = self.geo_at(rect, rect.right_bottom());
        (
            lat_bottom.min(lat_top) - 1.0,
            lat_bottom.max(lat_top) + 1.0,
            lon_left - 1.0,
            lon_right + 1.0,
        )
    }

    fn geo_at(&self, rect: Rect, pointer: Pos2) -> (f64, f64) {
        let (cx, cy) = project(self.center_lat, self.center_lon, self.zoom);
        let x = cx + (pointer.x - rect.center().x) as f64;
        let y = cy + (pointer.y - rect.center().y) as f64;
        unproject(x, y, self.zoom)
    }

    fn draw_graticule(&self, painter: &egui::Painter, rect: Rect) {
        let (lat_top, lon_left) = self.geo_at(rect, rect.left_top());
        let (lat_bottom, lon_right) = self.geo_at(rect, rect.right_bottom());
        let lat_from = lat_bottom.floor() as i32 - 1;
        let lat_to = lat_top.ceil() as i32 + 1;
        let lon_from = lon_left.floor() as i32 - 1;
        let lon_to = if lon_right < lon_left { 180 } else { lon_right.ceil() as i32 + 1 };

        let stroke = Stroke::new(1.0_f32, GRID);
        for lat in (lat_from..=lat_to).step_by(10) {
            let y = self.screen_of(rect, lat as f64, 0.0).y;
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], stroke);
        }
        for lon in (lon_from..=lon_to).step_by(10) {
            let p = self.screen_of(rect, 0.0, lon as f64).x;
            if rect.x_range().contains(p) {
                painter.line_segment(
                    [pos2(p, rect.top()), pos2(p, rect.bottom())],
                    stroke,
                );
            }
        }
    }

    fn draw_coastlines(&self, painter: &egui::Painter, rect: Rect, world: &World) {
        let stroke = Stroke::new(1.1_f32, COAST);
        let (lat_min, lat_max, lon_min, lon_max) = self.geo_bounds(rect);
        for ring in &world.land {
            if !ring.touches(lat_min, lat_max, lon_min, lon_max) {
                continue;
            }
            let points: Vec<Pos2> = ring
                .points
                .iter()
                .map(|p| self.screen_of(rect, p[1] as f64, p[0] as f64))
                .collect();
            draw_polyline(painter, &points, rect, stroke);
        }
    }

    fn draw_borders(&self, painter: &egui::Painter, rect: Rect, world: &World) {
        let stroke = Stroke::new(0.8_f32, Color32::from_rgb(54, 36, 40));
        let (lat_min, lat_max, lon_min, lon_max) = self.geo_bounds(rect);
        for line in &world.borders {
            if !line.touches(lat_min, lat_max, lon_min, lon_max) {
                continue;
            }
            let points: Vec<Pos2> = line
                .points
                .iter()
                .map(|p| self.screen_of(rect, p[1] as f64, p[0] as f64))
                .collect();
            draw_polyline(painter, &points, rect, stroke);
        }
    }

    fn draw_places(&self, painter: &egui::Painter, rect: Rect, world: &World) {
        for place in &world.places.list {
            if self.zoom < place.min_zoom || self.zoom > place.max_zoom || self.zoom > 6.2 {
                continue;
            }
            let p = self.screen_of(rect, place.lat, place.lon);
            if !rect.contains(p) {
                continue;
            }
            let size = if place.name.len() > 9 { 9.5 } else { 10.5 };
            painter.text(
                p,
                Align2::CENTER_CENTER,
                &place.name,
                FontId::proportional(size),
                Color32::from_rgba_unmultiplied(PLACE.r(), PLACE.g(), PLACE.b(), 200),
            );
        }
    }

    fn draw_airports(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        world: &World,
        matches: &[String],
        query: &str,
    ) {
        let searching = !query.trim().is_empty();
        let mut labels: Vec<Rect> = Vec::new();
        let mut drawn = 0usize;
        for airport in &world.airports.list {
            let hit = searching && matches.iter().any(|m| m == &airport.ident);
            // Large airports appear first, medium ones only when close enough:
            // a continent view must stay readable, like the reference map.
            let dot_zoom = if airport.class == 0 { 3.2 } else { 5.0 };
            if self.zoom < dot_zoom && !hit {
                continue;
            }
            let label_zoom = if airport.class == 0 { 4.2 } else { 6.1 };
            if self.zoom < label_zoom && !hit {
                continue;
            }
            let p = self.screen_of(rect, airport.lat, airport.lon);
            if !rect.expand(40.0).contains(p) {
                continue;
            }
            let color = if hit {
                Color32::from_rgb(255, 158, 160)
            } else if searching && !matches.is_empty() {
                Color32::from_rgba_unmultiplied(226, 120, 128, 70)
            } else {
                Color32::from_rgba_unmultiplied(226, 130, 138, 150)
            };
            painter.circle_filled(p, if hit { 3.4 } else { 2.2 }, color);
            if hit {
                painter.circle_stroke(p, 9.0, Stroke::new(1.2_f32, color));
            }
            if !self.show_labels || (searching && !matches.is_empty() && !hit) {
                continue;
            }
            let font = FontId::proportional(if hit { 10.5 } else { 9.0 });
            let text_color = if hit {
                Color32::from_rgb(255, 208, 208)
            } else {
                Color32::from_rgb(196, 170, 173)
            };
            let galley = painter.layout_no_wrap(airport.ident.clone(), font, text_color);
            let pos = p + vec2(5.0, -5.0);
            let box_rect = Rect::from_min_size(pos - vec2(1.0, 1.0), galley.size() + vec2(3.0, 2.0));
            if labels.iter().any(|other| other.intersects(box_rect)) {
                continue;
            }
            labels.push(box_rect);
            painter.galley(pos, galley, text_color);
            drawn += 1;
            if drawn > 160 {
                break;
            }
        }
    }

    fn draw_route(&self, painter: &egui::Painter, rect: Rect, plan: &FlightPlan) {
        let pts = plan.points();
        if pts.len() < 2 {
            return;
        }
        let mut screen: Vec<Pos2> = Vec::new();
        for pair in pts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let steps = ((haversine_nm(a.0, a.1, b.0, b.1) / 120.0).ceil() as usize).clamp(1, 64);
            for i in 0..steps {
                let t = i as f64 / steps as f64;
                let lat = a.0 + (b.0 - a.0) * t;
                let lon = a.1 + (b.1 - a.1) * t;
                screen.push(self.screen_of(rect, lat, lon));
            }
        }
        screen.push(self.screen_of(rect, pts[pts.len() - 1].0, pts[pts.len() - 1].1));

        painter.add(egui::Shape::line(
            screen,
            Stroke::new(2.4_f32, Color32::from_rgb(240, 82, 94)),
        ));

        if self.show_labels {
            for wp in &plan.route {
                if wp.id == plan.origin || wp.id == plan.dest {
                    continue; // drawn below as an airport
                }
                let p = self.screen_of(rect, wp.lat, wp.lon);
                if !rect.expand(40.0).contains(p) {
                    continue;
                }
                painter.circle_filled(p, 2.6, Color32::from_rgb(255, 150, 150));
                if self.zoom > 4.4 {
                    painter.text(
                        p + vec2(6.0, -6.0),
                        Align2::LEFT_BOTTOM,
                        &wp.id,
                        FontId::monospace(9.5),
                        Color32::from_rgb(226, 176, 180),
                    );
                }
            }
        }

        if let (Some(origin), Some(dest)) = (plan.origin_pos, plan.dest_pos) {
            for (pos, label, color) in [
                (origin, &plan.origin, GREEN),
                (dest, &plan.dest, ACCENT),
            ] {
                let p = self.screen_of(rect, pos.0, pos.1);
                if !rect.expand(60.0).contains(p) {
                    continue;
                }
                painter.circle_filled(p, 5.0, Color32::from_black_alpha(160));
                painter.circle_stroke(p, 5.0, Stroke::new(1.8_f32, color));
                painter.circle_filled(p, 2.0, color);
                if self.show_labels {
                    let label_pos = p + vec2(9.0, 0.0);
                    let galley = painter.layout_no_wrap(
                        label.clone(),
                        FontId::monospace(10.5),
                        color,
                    );
                    let bg = Rect::from_min_size(
                        label_pos + vec2(-3.0, -2.0),
                        galley.size() + vec2(6.0, 4.0),
                    );
                    painter.rect_filled(bg, CornerRadius::same(3), Color32::from_black_alpha(150));
                    painter.galley(label_pos, galley, color);
                }
            }
        }
    }

    fn draw_aircraft(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        lat: f64,
        lon: f64,
        heading: f64,
    ) {
        let p = self.screen_of(rect, lat, lon);
        if !rect.expand(40.0).contains(p) {
            return;
        }
        let angle = (if heading.is_finite() { heading } else { 0.0 } as f32).to_radians();
        let forward = vec2(angle.sin(), -angle.cos());
        let side = vec2(angle.cos(), angle.sin());
        let at = |along: f32, across: f32| p + forward * along + side * across;

        // A soft halo marks the aircraft without swallowing the map, and the
        // airliner silhouette keeps a nose, wings and a tailplane readable at
        // any zoom.
        painter.circle_filled(p, 17.0, Color32::from_rgba_unmultiplied(238, 66, 78, 24));
        painter.circle_stroke(
            p,
            15.0,
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(238, 66, 78, 64)),
        );

        let body = Color32::from_rgb(255, 246, 246);
        let wing = Color32::from_rgb(236, 226, 228);
        let edge = Stroke::new(1.0_f32, Color32::from_rgb(38, 20, 24));

        // Wings and tailplane first, then the fuselage on top of them.
        for flip in [1.0_f32, -1.0_f32] {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    at(2.4, 1.2 * flip),
                    at(0.4, 8.8 * flip),
                    at(-2.8, 8.8 * flip),
                    at(-3.6, 1.4 * flip),
                ],
                wing,
                edge,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    at(-7.2, 1.3 * flip),
                    at(-8.4, 5.2 * flip),
                    at(-10.8, 5.2 * flip),
                    at(-10.6, 1.3 * flip),
                ],
                wing,
                edge,
            ));
        }
        // Vertical fin, seen from above as a slim wedge.
        painter.add(egui::Shape::convex_polygon(
            vec![at(-8.8, 0.0), at(-12.4, 1.6), at(-12.4, -1.6)],
            wing,
            edge,
        ));
        painter.add(egui::Shape::convex_polygon(
            vec![
                at(12.0, 0.0),
                at(7.4, 1.7),
                at(-8.6, 1.9),
                at(-11.4, 0.0),
                at(-8.6, -1.9),
                at(7.4, -1.7),
            ],
            body,
            edge,
        ));
    }

    fn draw_scale(&self, painter: &egui::Painter, rect: Rect) {
        let center = rect.center();
        let (lat, lon) = self.geo_at(rect, pos2(center.x, rect.bottom() - 40.0));
        let (_, lon_east) = self.geo_at(rect, pos2(center.x + 120.0, rect.bottom() - 40.0));
        let span_nm = haversine_nm(lat, lon, lat, lon_east);
        if !span_nm.is_finite() || span_nm <= 0.1 {
            return;
        }
        let target = span_nm / 2.0;
        let nice = [1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 500.0, 1000.0]
            .into_iter()
            .find(|v| *v >= target)
            .unwrap_or(1000.0);
        let px = ((nice / span_nm) * 120.0) as f32;
        let y = rect.bottom() - 26.0;
        let x0 = rect.center().x - 40.0;
        painter.line_segment([pos2(x0, y), pos2(x0 + px, y)], Stroke::new(1.4_f32, MUTED));
        painter.line_segment([pos2(x0, y - 4.0), pos2(x0, y + 4.0)], Stroke::new(1.4_f32, MUTED));
        painter.line_segment(
            [pos2(x0 + px, y - 4.0), pos2(x0 + px, y + 4.0)],
            Stroke::new(1.4_f32, MUTED),
        );
        painter.text(
            pos2(x0 + px * 0.5, y - 6.0),
            Align2::CENTER_BOTTOM,
            format!("{nice:.0} NM"),
            FontId::monospace(9.5),
            MUTED,
        );
    }

    fn ensure_terrain(&mut self, ctx: &egui::Context, world: &World, rect: Rect) {
        // Slightly under native resolution: the vector coastlines stay crisp
        // while the fill texture keeps its memory footprint small.
        let pixels_per_point = (ctx.pixels_per_point() * 0.8).clamp(0.9, 1.6);
        let width = ((rect.width() * pixels_per_point).round() as usize).clamp(64, 2200);
        let height = ((rect.height() * pixels_per_point).round() as usize).clamp(64, 1400);
        let key = CacheKey {
            lat: (self.center_lat * 20_000.0) as i64,
            lon: (self.center_lon * 20_000.0) as i64,
            zoom: (self.zoom * 2000.0) as i64,
            width: width as u32,
            height: height as u32,
        };
        if let Some(cache) = &self.cache {
            if cache.key == key {
                return;
            }
        }

        let pixels_per_point_x = width as f64 / rect.width().max(1.0) as f64;
        let image = render_terrain(
            world,
            self.center_lat,
            self.center_lon,
            self.zoom,
            width,
            height,
            pixels_per_point_x,
        );

        match &mut self.cache {
            Some(cache) => cache.texture.set(image, egui::TextureOptions::LINEAR),
            None => {
                let texture =
                    ctx.load_texture("world-terrain", image, egui::TextureOptions::LINEAR);
                self.cache = Some(Cache { key, texture });
            }
        }
        if let Some(cache) = &mut self.cache {
            cache.key = key;
        }
    }
}

fn draw_polyline(painter: &egui::Painter, points: &[Pos2], rect: Rect, stroke: Stroke) {
    if points.len() < 2 {
        return;
    }
    let view = rect.expand(80.0);
    let mut run: Vec<Pos2> = Vec::with_capacity(points.len());
    for point in points {
        if view.contains(*point) {
            run.push(*point);
        } else if run.len() > 1 {
            painter.add(egui::Shape::line(std::mem::take(&mut run), stroke));
        } else {
            run.clear();
        }
    }
    if run.len() > 1 {
        painter.add(egui::Shape::line(run, stroke));
    }
}

/// Scanline-fills every land ring (even-odd, so lakes stay water) into an image.
fn render_terrain(
    world: &World,
    center_lat: f64,
    center_lon: f64,
    zoom: f64,
    width: usize,
    height: usize,
    scale: f64,
) -> egui::ColorImage {
    let (cx, cy) = project(center_lat, center_lon, zoom);
    let half_w = width as f64 / 2.0;
    let half_h = height as f64 / 2.0;
    let to_px = |lat: f64, lon: f64| -> (f64, f64) {
        let (x, y) = project(lat, lon, zoom);
        (
            (x - cx) * scale + half_w,
            (y - cy) * scale + half_h,
        )
    };

    let mut pixels = vec![0u8; width * height * 4];
    let ocean = [OCEAN.r(), OCEAN.g(), OCEAN.b(), 255];
    let land = [LAND.r(), LAND.g(), LAND.b(), 255];
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.copy_from_slice(&ocean);
    }

    let mut spans: Vec<Vec<f32>> = vec![Vec::new(); height];

    for ring in &world.land {
        if ring.points.len() < 3 {
            continue;
        }
        let (rx0, ry0) = to_px(ring.bounds[3] as f64, ring.bounds[0] as f64);
        let (rx1, ry1) = to_px(ring.bounds[1] as f64, ring.bounds[2] as f64);
        if rx1 < 0.0 || rx0 > width as f64 || ry1 < 0.0 || ry0 > height as f64 {
            continue;
        }
        let mut pts: Vec<(f64, f64)> = ring
            .points
            .iter()
            .map(|p| to_px(p[1] as f64, p[0] as f64))
            .collect();

        let (min_x, max_x, min_y, max_y) = pts.iter().fold(
            (f64::MAX, f64::MIN, f64::MAX, f64::MIN),
            |acc, p| {
                (
                    acc.0.min(p.0),
                    acc.1.max(p.0),
                    acc.2.min(p.1),
                    acc.3.max(p.1),
                )
            },
        );
        if max_x < 0.0 || min_x > width as f64 || max_y < 0.0 || min_y > height as f64 {
            continue;
        }
        if pts.first() != pts.last() {
            if let Some(first) = pts.first().copied() {
                pts.push(first);
            }
        }

        for pair in pts.windows(2) {
            let (x0, y0) = pair[0];
            let (x1, y1) = pair[1];
            if !x0.is_finite() || !y0.is_finite() || !x1.is_finite() || !y1.is_finite() {
                continue;
            }
            if (x0 - x1).abs() > width as f64 * 0.5 {
                continue; // antimeridian jump
            }
            if (y0 - y1).abs() < f64::EPSILON {
                continue;
            }
            let (top, bottom, top_x, bottom_x) = if y0 < y1 {
                (y0, y1, x0, x1)
            } else {
                (y1, y0, x1, x0)
            };
            let start = top.ceil().max(0.0) as i64;
            let end = bottom.floor().min((height - 1) as f64) as i64;
            if end < start {
                continue;
            }
            let slope = (bottom_x - top_x) / (bottom - top);
            for y in start..=end {
                // Clamping keeps the crossing count even: dropping off-screen
                // crossings would flip the inside/outside parity of the row.
                let x = (top_x + (y as f64 - top) * slope).clamp(0.0, width as f64);
                spans[y as usize].push(x as f32);
            }
        }
    }

    for (y, row) in spans.iter_mut().enumerate() {
        if row.len() < 2 {
            continue;
        }
        row.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut i = 0;
        while i + 1 < row.len() {
            let start = row[i].max(0.0) as usize;
            let end = (row[i + 1].min(width as f32) as usize).min(width);
            if end > start {
                let base = y * width * 4;
                let from = base + start * 4;
                let to = base + end * 4;
                for pixel in pixels[from..to].chunks_exact_mut(4) {
                    pixel.copy_from_slice(&land);
                }
            }
            i += 2;
        }
    }

    egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterises_land_and_water() {
        let world = World::embedded();
        assert!(world.land.len() > 100, "land data should be embedded");

        let started = std::time::Instant::now();
        let image = render_terrain(&world, 43.0, 11.0, 5.0, 1200, 700, 1.0);
        let elapsed = started.elapsed();

        let mut land = 0usize;
        let mut ocean = 0usize;
        for pixel in &image.pixels {
            if *pixel == LAND {
                land += 1;
            } else if *pixel == OCEAN {
                ocean += 1;
            }
        }
        println!("terrain rendered in {elapsed:?}: land={land} ocean={ocean}");
        if std::env::var("MSFS_MAP_DUMP").is_ok() {
            let mut buffer = image::RgbaImage::new(1200, 700);
            for (index, pixel) in image.pixels.iter().enumerate() {
                buffer.put_pixel(
                    (index % 1200) as u32,
                    (index / 1200) as u32,
                    image::Rgba([pixel.r(), pixel.g(), pixel.b(), 255]),
                );
            }
            let _ = buffer.save("map_dump.png");
        }
        assert!(land > 40_000, "land missing from the Italy view");
        assert!(ocean > 10_000, "the Mediterranean should stay water");
        assert!(elapsed.as_millis() < 400, "terrain fill must stay fast");
    }
}
