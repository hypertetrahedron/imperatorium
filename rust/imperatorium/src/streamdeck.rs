//! `imperatorium streamdeck`: one key per session, coloured by what that
//! session is doing - `streamdeck_surface.py` without Python, pillow or a
//! vendored hidapi.dll (phase 5). It holds no state of its own and talks to
//! the dispatcher over HTTP, so it can be restarted or killed at any time.
//!
//! Colours: orange blocked (waiting on you), blue working, green idle, grey
//! no session running for a pinned project. Every open session gets a key; a
//! project with several shows `1/2` in the corner. Pinned projects
//! (config.json `streamdeck.keys`) come first and keep a grey key while
//! nothing runs there, unless keys run short. A key whose session changes
//! state flashes old/new for FLASH_SECONDS. A press focuses that session,
//! or opens one. A parked permission request turns the last three keys into
//! a decision - what is asking, ALLOW, DENY - armed only after ARM_SECONDS,
//! so a press meant for a project key cannot land on ALLOW.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::pyish::{py_str, truthy};

type Row = Map<String, Value>;

const POLL_SECONDS: f64 = 1.0;
const ARM_SECONDS: f64 = 1.0;
const DECISION_KEYS: usize = 3;
const FLASH_SECONDS: f64 = 5.0;
const FLASH_HALF: f64 = 0.5;

type Rgb = [u8; 3];
const INK: Rgb = [245, 245, 238];

fn colours(state: &str) -> (Rgb, Rgb) {
    match state {
        "blocked" | "ask" => ([176, 84, 30], INK),
        "working" => ([45, 92, 138], INK),
        "complete" => ([112, 62, 160], INK),
        "tasks" | "idle" => ([47, 107, 79], INK),
        "ready" => ([52, 52, 46], [190, 190, 182]),
        "offline" => ([60, 24, 24], [200, 170, 170]),
        "allow" => ([32, 120, 64], INK),
        "deny" => ([150, 30, 40], INK),
        _ => ([38, 38, 33], [140, 140, 132]),
    }
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn s<'a>(row: &'a Row, k: &str) -> Option<&'a str> {
    row.get(k).and_then(Value::as_str)
}

/// state_for: what colour a session's key is.
pub fn state_for(row: Option<&Row>) -> &'static str {
    let Some(row) = row else { return "none" };
    if truthy(row.get("attention")) {
        return if s(row, "attention_kind") == Some("complete") { "complete" } else { "blocked" };
    }
    if s(row, "status") == Some("busy") || s(row, "state") == Some("working") {
        return "working";
    }
    if s(row, "status") == Some("idle")
        && (truthy(row.get("waiting_tasks")) || truthy(row.get("background_tasks")))
    {
        return "tasks"; // between turns, but something it started still runs
    }
    if s(row, "status") == Some("idle") {
        return "ready";
    }
    "none"
}

/// wrap: a project name on short lines a 72 px key can show.
pub fn wrap(label: &str, width: usize) -> Vec<String> {
    let cleaned = label.replace(['_', '-'], " ");
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in cleaned.split_whitespace() {
        let n = word.chars().count();
        if n > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            lines.push(word.chars().take(width - 1).collect::<String>() + "\u{2026}");
            continue;
        }
        let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
        if candidate.chars().count() <= width {
            current = candidate;
        } else {
            if !current.is_empty() {
                lines.push(current);
            }
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines.truncate(3);
    if lines.is_empty() {
        lines.push(label.chars().take(width).collect());
    }
    lines
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub id: String,
    pub project: String,
    pub label: String,
    pub row: Option<Row>,
    pub count: Option<String>,
}

/// What a key shows: (label, state, badge, desktop app, count).
type Look = (String, String, Option<String>, bool, Option<String>);

pub struct Surface {
    keys: usize,
    pinned: Vec<String>,
    pub slots: Vec<Slot>,
    last: HashMap<usize, Look>,
    seen: HashMap<String, String>,
    flashes: HashMap<String, (String, String, f64)>,
    decision: Option<Row>,
    decision_at: f64,
}

impl Surface {
    pub fn new(keys: usize, pinned: Vec<String>) -> Self {
        Surface {
            keys,
            pinned,
            slots: vec![],
            last: HashMap::new(),
            seen: HashMap::new(),
            flashes: HashMap::new(),
            decision: None,
            decision_at: 0.0,
        }
    }

    fn project_keys(&self) -> usize {
        if self.decision.is_some() {
            self.keys.saturating_sub(DECISION_KEYS)
        } else {
            self.keys
        }
    }

    /// The oldest parked request without questions (three keys cannot
    /// choose answers, and the dispatcher refuses a bare Allow for one).
    pub fn set_decision(&mut self, pending: &[Value], at: f64) {
        let oldest = pending
            .iter()
            .filter_map(Value::as_object)
            .find(|p| !truthy(p.get("questions")))
            .cloned();
        let id = |d: &Option<Row>| d.as_ref().and_then(|d| d.get("id").cloned());
        if id(&oldest) != id(&self.decision) {
            self.decision_at = at;
        }
        self.decision = oldest;
    }

    fn decision_slot(&self, key: usize) -> Option<&'static str> {
        self.decision.as_ref()?;
        let first = self.keys.checked_sub(DECISION_KEYS)?;
        (key >= first).then(|| ["info", "allow", "deny"][key - first])
    }

    /// The verdict a press on `key` means, or None.
    pub fn decide_press(&self, key: usize, at: f64) -> Option<Value> {
        let slot = self.decision_slot(key)?;
        if !matches!(slot, "allow" | "deny") || at - self.decision_at < ARM_SECONDS {
            return None;
        }
        Some(json!({"id": self.decision.as_ref()?.get("id").cloned().unwrap_or(Value::Null),
                    "verdict": slot, "who": "streamdeck"}))
    }

    /// Surface.layout: the slots, one per key, in key order.
    pub fn layout(&self, sessions: &[Row], by_path: &HashMap<String, String>) -> Vec<Slot> {
        let mut by_project: indexmap::IndexMap<String, Vec<Row>> = indexmap::IndexMap::new();
        for row in sessions {
            let Some(cwd) = s(row, "cwd").filter(|c| !c.is_empty()) else { continue };
            if truthy(row.get("remote")) || state_for(Some(row)) == "none" {
                continue;
            }
            // A worktree session belongs on its project's key.
            let name = if truthy(row.get("worktree")) && truthy(row.get("project")) {
                py_str(&row["project"])
            } else if let Some(n) = crate::paths::abs_normcase(cwd).and_then(|p| by_path.get(&p).cloned()) {
                n
            } else {
                let base = cwd.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("");
                if base.is_empty() { cwd.to_string() } else { base.to_string() }
            };
            by_project.entry(name).or_default().push(row.clone());
        }
        for rows in by_project.values_mut() {
            rows.sort_by(|a, b| {
                let k = |r: &Row| (r.get("startedAt").and_then(Value::as_f64).unwrap_or(0.0),
                                   r.get("sessionId").map(py_str).unwrap_or_default());
                k(a).partial_cmp(&k(b)).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        let expand = |name: &str| -> Vec<Slot> {
            match by_project.get(name).filter(|r| !r.is_empty()) {
                None => vec![Slot { id: format!("project:{name}"), project: name.into(),
                                    label: name.into(), row: None, count: None }],
                Some(rows) => rows
                    .iter()
                    .enumerate()
                    .map(|(n, r)| Slot {
                        id: r.get("sessionId").map(py_str).unwrap_or_default(),
                        project: name.into(),
                        label: name.into(),
                        row: Some(r.clone()),
                        count: (rows.len() > 1).then(|| format!("{}/{}", n + 1, rows.len())),
                    })
                    .collect(),
            }
        };
        let mut slots = Vec::new();
        for name in &self.pinned {
            slots.extend(expand(name));
        }
        let mut extras: Vec<&String> = by_project.keys().filter(|n| !self.pinned.contains(n)).collect();
        extras.sort_by_key(|n| {
            let blocked = by_project[*n].iter().any(|r| truthy(r.get("attention")));
            (!blocked, n.to_lowercase())
        });
        for name in extras {
            slots.extend(expand(name));
        }
        let room = self.project_keys();
        while slots.len() > room {
            let Some(i) = slots.iter().rposition(|sl| sl.row.is_none()) else { break };
            slots.remove(i);
        }
        slots.truncate(room);
        slots
    }

    /// Surface.flash_state: the state to paint now, a change starting a flash.
    fn flash_state(&mut self, id: &str, state: &str, at: f64) -> String {
        let before = self.seen.insert(id.to_string(), state.to_string());
        if let Some(b) = before.filter(|b| b != state) {
            self.flashes.insert(id.to_string(), (b, state.to_string(), at));
        }
        let Some((old, new, t)) = self.flashes.get(id).cloned() else {
            return state.to_string();
        };
        if at - t >= FLASH_SECONDS {
            self.flashes.remove(id);
            return state.to_string();
        }
        if ((at - t) / FLASH_HALF) as i64 % 2 == 1 { old } else { new }
    }

    pub fn flashing(&self) -> bool {
        !self.flashes.is_empty()
    }

    /// Surface.paint: what changed, as (key, look) to draw.
    pub fn paint(&mut self, slots: Vec<Slot>, at: f64) -> Vec<(usize, Look)> {
        self.slots = slots;
        let mut out = Vec::new();
        let mut shown = HashSet::new();
        for key in 0..self.keys {
            let look: Look = if let Some(slot) = self.decision_slot(key) {
                let d = self.decision.as_ref().unwrap();
                let cwd = d.get("cwd").map(py_str).unwrap_or_default();
                let base = cwd.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("").to_string();
                let tool = d.get("tool").filter(|t| truthy(Some(t))).map(py_str).unwrap_or_else(|| "?".into());
                match slot {
                    "info" => (format!("{tool} {base}"), "ask".into(), None, false, None),
                    "allow" => ("ALLOW".into(), "allow".into(), None, false, None),
                    _ => ("DENY".into(), "deny".into(), None, false, None),
                }
            } else if let Some(sl) = self.slots.get(key).cloned() {
                shown.insert(sl.id.clone());
                let state = self.flash_state(&sl.id, state_for(sl.row.as_ref()), at);
                let row = sl.row.as_ref();
                let badge = row
                    .filter(|r| truthy(r.get("attention")))
                    .and_then(|r| r.get("attention_since").and_then(Value::as_f64))
                    .filter(|t| *t != 0.0)
                    .map(|t| format!("{}m", (((at - t) / 60.0).floor() as i64).max(0)));
                let app = row.is_some_and(|r| truthy(r.get("desktop_app")));
                (sl.label.clone(), state, badge, app, sl.count.clone())
            } else {
                (String::new(), "none".into(), None, false, None)
            };
            if self.last.get(&key) != Some(&look) {
                self.last.insert(key, look.clone());
                out.push((key, look));
            }
        }
        // A session that leaves the board starts fresh if it comes back.
        let gone: Vec<String> = self.seen.keys().filter(|k| !shown.contains(*k)).cloned().collect();
        for g in gone {
            self.seen.remove(&g);
            self.flashes.remove(&g);
        }
        out
    }

    /// Forget what was painted, so the next paint redraws every key.
    pub fn repaint_all(&mut self) {
        self.last.clear();
    }

    /// on_press: the API call a press means - (path, payload), or None.
    pub fn press(&self, key: usize, at: f64) -> Option<(&'static str, Value)> {
        if let Some(v) = self.decide_press(key, at) {
            return Some(("/api/decide", v));
        }
        if self.decision_slot(key).is_some() {
            return None; // the info key, or a decision key not yet armed
        }
        let sl = self.slots.get(key)?;
        let mut payload = json!({"project": sl.project});
        if let Some(r) = &sl.row {
            payload["session_id"] = r.get("sessionId").cloned().unwrap_or(Value::Null);
        }
        Some(("/api/focus", payload))
    }
}

// -- drawing --------------------------------------------------------------

struct Fonts {
    font: Option<ab_glyph::FontVec>,
}

impl Fonts {
    /// Segoe UI is on every Windows install; Arial, then DejaVu otherwise.
    fn load() -> Self {
        let mut dirs = Vec::new();
        if let Some(w) = crate::paths::env_nonempty("WINDIR") {
            dirs.push(std::path::PathBuf::from(w).join("Fonts"));
        }
        dirs.push("/usr/share/fonts/truetype/dejavu".into());
        for dir in &dirs {
            for name in ["segoeui.ttf", "arial.ttf", "DejaVuSans.ttf"] {
                if let Ok(bytes) = std::fs::read(dir.join(name)) {
                    if let Ok(f) = ab_glyph::FontVec::try_from_vec(bytes) {
                        return Fonts { font: Some(f) };
                    }
                }
            }
        }
        Fonts { font: None }
    }

    /// PIL's truetype size is the em in pixels; ab_glyph scales by height.
    fn scale(&self, em: f32) -> ab_glyph::PxScale {
        use ab_glyph::Font;
        match &self.font {
            Some(f) => {
                let upem = f.units_per_em().unwrap_or(1000.0);
                ab_glyph::PxScale::from(em * f.height_unscaled() / upem)
            }
            None => ab_glyph::PxScale::from(em),
        }
    }

    fn width(&self, text: &str, em: f32) -> f32 {
        use ab_glyph::{Font, ScaleFont};
        let Some(f) = &self.font else { return text.len() as f32 * em * 0.5 };
        let sf = f.as_scaled(self.scale(em));
        text.chars().map(|c| sf.h_advance(sf.glyph_id(c))).sum()
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(&self, img: &mut image::RgbImage, text: &str, x: f32, y: f32, em: f32, fg: Rgb, bg: Rgb) {
        use ab_glyph::{Font, ScaleFont};
        let Some(f) = &self.font else { return };
        let scale = self.scale(em);
        let sf = f.as_scaled(scale);
        let mut caret = x;
        for c in text.chars() {
            let id = sf.glyph_id(c);
            let glyph = id.with_scale_and_position(scale, ab_glyph::point(caret, y + sf.ascent()));
            caret += sf.h_advance(id);
            if let Some(og) = f.outline_glyph(glyph) {
                let b = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let px = b.min.x as i32 + gx as i32;
                    let py = b.min.y as i32 + gy as i32;
                    if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                        let p = img.get_pixel_mut(px as u32, py as u32);
                        for i in 0..3 {
                            p[i] = (fg[i] as f32 * cov + bg[i] as f32 * (1.0 - cov)).round() as u8;
                        }
                    }
                });
            }
        }
    }
}

/// render_key: a key's image.
fn render(fonts: &Fonts, size: (u32, u32), look: &Look) -> image::DynamicImage {
    let (label, state, badge, app, count) = look;
    let (bg, fg) = colours(state);
    let (w, h) = size;
    let mut img = image::RgbImage::from_pixel(w, h, image::Rgb(bg));
    if *app {
        // A small window glyph: the session lives in the Claude desktop app.
        let (x, y, gw, gh) = (5u32, 5u32, 14u32, 11u32);
        for i in x..=x + gw {
            for j in [y, y + 3, y + gh] {
                img.put_pixel(i, j, image::Rgb(fg));
            }
        }
        for j in y..=y + gh {
            img.put_pixel(x, j, image::Rgb(fg));
            img.put_pixel(x + gw, j, image::Rgb(fg));
        }
    }
    if !label.is_empty() {
        let lines = wrap(label, 9);
        let em = if lines.len() < 3 { 13.0 } else { 11.0 };
        let line_h = em + 2.0;
        let mut y = ((h as f32 - line_h * lines.len() as f32) / 2.0).floor();
        for line in &lines {
            let lw = fonts.width(line, em);
            fonts.draw(&mut img, line, ((w as f32 - lw) / 2.0).floor(), y, em, fg, bg);
            y += line_h;
        }
    }
    if let Some(c) = count {
        let cw = fonts.width(c, 10.0);
        fonts.draw(&mut img, c, w as f32 - cw - 4.0, 3.0, 10.0, fg, bg);
    }
    if let Some(b) = badge {
        let bw = fonts.width(b, 10.0);
        fonts.draw(&mut img, b, w as f32 - bw - 4.0, h as f32 - 10.0 - 4.0, 10.0, fg, bg);
    }
    image::DynamicImage::ImageRgb8(img)
}

// -- talking to the dispatcher --------------------------------------------

fn base() -> String {
    crate::paths::env_nonempty("IMPERATORIUM_ENDPOINT_BASE").unwrap_or_else(|| "http://127.0.0.1:8792".into())
}

fn api(path: &str, payload: Option<&Value>) -> Result<Value, String> {
    let t = Duration::from_secs(6);
    let agent = ureq::AgentBuilder::new().timeout_connect(t).timeout_read(t).build();
    let url = format!("{}{path}", base());
    let resp = match payload {
        Some(p) => agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(&p.to_string()),
        None => agent.get(&url).call(),
    };
    let text = resp.map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

struct Board {
    sessions: Vec<Row>,
    pending: Vec<Value>,
    by_path: HashMap<String, String>,
}

/// One poll: sessions, parked requests, the catalogue. Err while the
/// dispatcher is restarting - the keys say so rather than the surface exit.
fn poll() -> Result<Board, String> {
    let sessions = api("/api/sessions", None)?
        .get("sessions")
        .and_then(Value::as_array)
        .ok_or("no sessions")?
        .iter()
        .filter_map(|r| r.as_object().cloned())
        .collect();
    let pending = api("/api/pending", None)?
        .get("pending")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut by_path = HashMap::new();
    for p in api("/api/projects", None)?
        .get("projects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        if let (Some(path), Some(name)) = (p.get("path").and_then(Value::as_str), p.get("name").and_then(Value::as_str)) {
            if p.get("host").is_some_and(|h| truthy(Some(h))) {
                continue;
            }
            if let Some(k) = crate::paths::abs_normcase(path) {
                by_path.insert(k, name.to_string());
            }
        }
    }
    Ok(Board { sessions, pending, by_path })
}

fn pinned(home: Option<&str>) -> Vec<String> {
    let path = crate::paths::config_home(home).join("config.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|c| c.get("streamdeck")?.get("keys")?.as_array().cloned())
        .map(|k| k.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// `imperatorium streamdeck [--brightness N] [--once] [--home DIR]`.
pub fn main(args: &[String], home: Option<&str>) -> i32 {
    let mut brightness = 60u8;
    let mut once = false;
    let mut preview = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--brightness" => brightness = it.next().and_then(|v| v.parse().ok()).unwrap_or(60),
            "--once" => once = true,
            // One frame as PNGs (key00.png ...), no deck needed: 15 keys of 72 px.
            "--preview" => preview = it.next().cloned(),
            _ => {}
        }
    }
    if let Some(dir) = preview {
        let fonts = Fonts::load();
        let mut surface = Surface::new(15, pinned(home));
        let board = match poll() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("streamdeck: dispatcher unreachable: {e}");
                return 1;
            }
        };
        surface.set_decision(&board.pending, now() - 10.0);
        let slots = surface.layout(&board.sessions, &board.by_path);
        let _ = std::fs::create_dir_all(&dir);
        for (key, look) in surface.paint(slots, now()) {
            let path = std::path::Path::new(&dir).join(format!("key{key:02}.png"));
            if let Err(e) = render(&fonts, (72, 72), &look).save(&path) {
                eprintln!("{}: {e}", path.display());
            }
            println!("key {key:2}: {:?}", look);
        }
        return 0;
    }
    let hid = match elgato_streamdeck::new_hidapi() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("streamdeck: {e}");
            return 1;
        }
    };
    let Some((kind, serial)) = elgato_streamdeck::list_devices(&hid).into_iter().next() else {
        eprintln!("no Stream Deck found");
        return 1;
    };
    let deck = match elgato_streamdeck::StreamDeck::connect(&hid, kind, &serial) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("streamdeck: {e:?}");
            return 1;
        }
    };
    let _ = deck.reset();
    let _ = deck.set_brightness(brightness);
    let keys = kind.key_count() as usize;
    let size = kind.key_image_format().size;
    let size = (size.0 as u32, size.1 as u32);
    println!("{kind:?}, {keys} keys");
    let fonts = Fonts::load();
    let mut surface = Surface::new(keys, pinned(home));

    // Polls on their own thread: a slow dispatcher never stalls the keys.
    let (tx, rx) = mpsc::channel::<Result<Board, String>>();
    std::thread::spawn(move || loop {
        if tx.send(poll()).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_secs_f64(POLL_SECONDS));
    });
    let mut slots: Vec<Slot> = vec![];
    let mut down = vec![false; keys];
    loop {
        let mut fresh = false;
        while let Ok(got) = rx.try_recv() {
            fresh = true;
            match got {
                Ok(b) => {
                    surface.set_decision(&b.pending, now());
                    slots = surface.layout(&b.sessions, &b.by_path);
                }
                Err(_) => {
                    // A decision that can no longer be delivered stops being offered.
                    surface.set_decision(&[], now());
                    slots = vec![];
                }
            }
        }
        if fresh || surface.flashing() {
            for (key, look) in surface.paint(slots.clone(), now()) {
                if let Err(e) = deck.set_button_image(key as u8, render(&fonts, size, &look)) {
                    eprintln!("key {key}: {e:?}");
                    surface.repaint_all();
                }
            }
            let _ = deck.flush();
            if once && fresh {
                return 0; // --once: one frame from one poll
            }
        }
        match deck.read_input(Some(Duration::from_millis(100))) {
            Ok(elgato_streamdeck::StreamDeckInput::ButtonStateChange(states)) => {
                for (key, pressed) in states.iter().enumerate().take(keys) {
                    if *pressed && !down[key] {
                        if let Some((path, payload)) = surface.press(key, now()) {
                            let label = surface.slots.get(key).map(|s| s.project.clone()).unwrap_or_default();
                            std::thread::spawn(move || match api(path, Some(&payload)) {
                                Ok(r) => println!("press {key:<2} {label:<24} -> {}",
                                    r.get("action").or(r.get("error")).or(r.get("ok")).map(py_str).unwrap_or_default()),
                                Err(e) => println!("press {key:<2} {label:<24} -> dispatcher unreachable: {e}"),
                            });
                        }
                    }
                    down[key] = *pressed;
                }
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("streamdeck: {e:?}; the deck went away");
                return 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(v: Value) -> Row {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn wraps_like_python() {
        assert_eq!(wrap("gem-trip", 9), ["gem trip"]);
        assert_eq!(wrap("imperatorium", 9), ["imperato\u{2026}"]);
        assert_eq!(wrap("ai_local_operations", 9), ["ai local", "operatio\u{2026}"]);
        assert_eq!(wrap("a b c d e f g h i j k l m n o", 3).len(), 3);
    }

    #[test]
    fn states() {
        assert_eq!(state_for(None), "none");
        assert_eq!(state_for(Some(&row(json!({"attention": true, "status": "busy"})))), "blocked");
        assert_eq!(state_for(Some(&row(json!({"status": "idle", "background_tasks": 1})))), "tasks");
        assert_eq!(state_for(Some(&row(json!({"status": "idle", "waiting_tasks": 2})))), "tasks");
        assert_eq!(state_for(Some(&row(json!({"attention": true, "attention_kind": "complete"})))), "complete");
        assert_eq!(state_for(Some(&row(json!({"status": "idle"})))), "ready");
    }

    #[test]
    fn layout_pinned_first_then_blocked_then_by_name() {
        let s = Surface::new(5, vec!["pinned".into(), "empty".into()]);
        let rows = vec![
            row(json!({"sessionId": "a", "cwd": "C:\\x\\zeta", "status": "idle"})),
            row(json!({"sessionId": "b", "cwd": "C:\\x\\alpha", "status": "idle"})),
            row(json!({"sessionId": "c", "cwd": "C:\\x\\blocked", "status": "idle", "attention": true})),
            row(json!({"sessionId": "d", "cwd": "C:\\x\\pinned", "status": "busy", "startedAt": 2})),
            row(json!({"sessionId": "e", "cwd": "C:\\x\\pinned", "status": "busy", "startedAt": 1})),
            row(json!({"sessionId": "r", "cwd": "/home/x", "status": "idle", "remote": true})),
        ];
        let got: Vec<_> = s.layout(&rows, &HashMap::new()).into_iter().map(|sl| (sl.id, sl.count)).collect();
        // 6 slots for 5 keys: the empty pinned project gives way first.
        assert_eq!(got, vec![
            ("e".to_string(), Some("1/2".to_string())),
            ("d".to_string(), Some("2/2".to_string())),
            ("c".to_string(), None),
            ("b".to_string(), None),
            ("a".to_string(), None),
        ]);
    }

    #[test]
    fn decisions_arm_after_a_moment() {
        let mut s = Surface::new(15, vec![]);
        s.set_decision(&[json!({"id": "q", "questions": [1]}), json!({"id": "p1", "tool": "Bash"})], 100.0);
        assert_eq!(s.decide_press(13, 100.5), None);
        assert_eq!(s.decide_press(13, 101.5).unwrap()["verdict"], "allow");
        assert_eq!(s.decide_press(14, 101.5).unwrap()["id"], "p1");
        assert_eq!(s.decide_press(12, 101.5), None); // the info key
        assert_eq!(s.press(12, 101.5), None);
    }

    #[test]
    fn a_change_flashes() {
        let mut s = Surface::new(1, vec![]);
        let busy = vec![Slot { id: "x".into(), project: "p".into(), label: "p".into(),
                               row: Some(row(json!({"status": "busy"}))), count: None }];
        let idle = vec![Slot { row: Some(row(json!({"status": "idle"}))), ..busy[0].clone() }];
        assert_eq!(s.paint(busy, 0.0)[0].1 .1, "working");
        assert_eq!(s.paint(idle.clone(), 10.0)[0].1 .1, "ready");
        assert_eq!(s.paint(idle.clone(), 10.6)[0].1 .1, "working");
        assert!(s.flashing());
        s.paint(idle.clone(), 16.0);
        assert!(!s.flashing());
    }
}

/// test_streamdeck.py, ported: layout, labelling, state mapping, flashing,
/// decisions and the desktop-app glyph.
#[cfg(test)]
mod surface_tests {
    use super::*;

    fn row(cwd: &str, extra: Value) -> Row {
        let mut r = json!({"sessionId": format!("s-{cwd}"), "cwd": cwd, "status": "idle"})
            .as_object().unwrap().clone();
        for (k, v) in extra.as_object().unwrap() {
            r.insert(k.clone(), v.clone());
        }
        r
    }

    fn by_path() -> HashMap<String, String> {
        [("Y:/projects-software/laser-ledger", "laser-ledger"),
         ("Y:/projects-software/gem-trip", "gem-trip"),
         ("Y:/projects-software/amarantha", "amarantha")]
            .iter()
            .map(|(p, n)| (crate::paths::abs_normcase(p).unwrap(), n.to_string()))
            .collect()
    }

    fn names(slots: &[Slot]) -> Vec<&str> {
        slots.iter().map(|s| s.project.as_str()).collect()
    }

    fn pinned(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    fn st(v: Value) -> &'static str {
        state_for(Some(v.as_object().unwrap()))
    }

    // -- Labels -------------------------------------------------------------

    #[test]
    fn hyphens_become_word_breaks() {
        assert_eq!(wrap("laser-ledger", 9), ["laser", "ledger"]);
    }

    #[test]
    fn long_single_word_is_truncated_with_an_ellipsis() {
        let lines = wrap("bookoferrantpages", 9);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with('\u{2026}'));
        assert!(lines[0].chars().count() <= 9);
    }

    #[test]
    fn never_more_than_three_lines() {
        assert!(wrap("a b c d e f g h i j k", 9).len() <= 3);
        assert!(wrap("aaaaaaaa bbbbbbbb cccccccc dddddddd eeeeeeee", 9).len() <= 3);
    }

    #[test]
    fn empty_label_still_returns_a_line() {
        assert_eq!(wrap("", 9).len(), 1);
    }

    // -- States -------------------------------------------------------------

    #[test]
    fn no_session_is_none() {
        assert_eq!(state_for(None), "none");
    }

    #[test]
    fn attention_outranks_busy() {
        assert_eq!(st(json!({"status": "busy", "attention": true})), "blocked");
    }

    #[test]
    fn busy_and_working_both_read_as_working() {
        assert_eq!(st(json!({"status": "busy"})), "working");
        assert_eq!(st(json!({"state": "working"})), "working");
    }

    #[test]
    fn idle_with_nothing_is_ready() {
        assert_eq!(st(json!({"status": "idle"})), "ready");
    }

    #[test]
    fn idle_waiting_on_a_background_task_is_tasks() {
        assert_eq!(st(json!({"status": "idle", "background_tasks": 1})), "tasks");
        assert_eq!(st(json!({"status": "idle", "waiting_tasks": 2})), "tasks");
    }

    #[test]
    fn complete_is_its_own_colour() {
        assert_eq!(st(json!({"attention": true, "attention_kind": "complete"})), "complete");
    }

    #[test]
    fn finished_background_job_is_not_shown_as_live() {
        assert_eq!(st(json!({"state": "done"})), "none");
    }

    // -- Layout -------------------------------------------------------------

    #[test]
    fn pinned_keys_keep_their_position() {
        let s = Surface::new(15, pinned(&["laser-ledger", "gem-trip"]));
        let slots = s.layout(&[row("Y:/projects-software/amarantha", json!({}))], &by_path());
        assert_eq!(names(&slots), ["laser-ledger", "gem-trip", "amarantha"]);
        assert!(slots[0].row.is_none());
    }

    #[test]
    fn unpinned_blocked_project_sorts_ahead_of_other_extras() {
        let s = Surface::new(15, vec![]);
        let slots = s.layout(&[row("Y:/projects-software/amarantha", json!({})),
                               row("Y:/projects-software/gem-trip", json!({"attention": true}))], &by_path());
        assert_eq!(slots[0].project, "gem-trip");
    }

    #[test]
    fn order_is_capped_at_the_key_count() {
        let s = Surface::new(2, pinned(&["a", "b", "c", "d"]));
        assert_eq!(s.layout(&[], &by_path()).len(), 2);
    }

    #[test]
    fn every_session_of_a_project_gets_a_key() {
        let s = Surface::new(15, vec![]);
        let slots = s.layout(&[row("Y:/projects-software/gem-trip", json!({"sessionId": "late", "startedAt": 2})),
                               row("Y:/projects-software/gem-trip", json!({"sessionId": "early", "startedAt": 1}))],
                             &by_path());
        assert_eq!(slots.iter().map(|sl| sl.id.as_str()).collect::<Vec<_>>(), ["early", "late"]);
        assert_eq!(slots.iter().map(|sl| sl.count.clone()).collect::<Vec<_>>(),
                   [Some("1/2".to_string()), Some("2/2".to_string())]);
    }

    #[test]
    fn a_lone_session_has_no_counter() {
        let s = Surface::new(15, vec![]);
        let slots = s.layout(&[row("Y:/projects-software/gem-trip", json!({}))], &by_path());
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].count, None);
    }

    #[test]
    fn pinned_project_expands_in_place() {
        let s = Surface::new(15, pinned(&["gem-trip", "laser-ledger"]));
        let slots = s.layout(&[row("Y:/projects-software/gem-trip", json!({"sessionId": "a"})),
                               row("Y:/projects-software/gem-trip", json!({"sessionId": "b"}))], &by_path());
        assert_eq!(names(&slots), ["gem-trip", "gem-trip", "laser-ledger"]);
    }

    #[test]
    fn open_sessions_beat_pinned_projects_with_none() {
        let s = Surface::new(3, pinned(&["laser-ledger", "amarantha"]));
        let slots = s.layout(&[row("Y:/projects-software/gem-trip", json!({"sessionId": "a"})),
                               row("Y:/projects-software/gem-trip", json!({"sessionId": "b"}))], &by_path());
        assert_eq!(names(&slots), ["laser-ledger", "gem-trip", "gem-trip"]);
        assert!(slots[1..].iter().all(|sl| sl.row.is_some()));
    }

    #[test]
    fn finished_and_remote_sessions_get_no_key() {
        let s = Surface::new(15, vec![]);
        let slots = s.layout(&[row("Y:/projects-software/gem-trip", json!({"status": null, "state": "done"})),
                               row("Y:/projects-software/amarantha", json!({"remote": true}))], &by_path());
        assert!(slots.is_empty());
    }

    #[test]
    fn sessions_outside_the_catalogue_are_named_by_folder() {
        let s = Surface::new(15, vec![]);
        let slots = s.layout(&[row("C:/somewhere/else", json!({}))], &by_path());
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].label, "else");
    }

    #[test]
    fn a_press_on_an_empty_slot_does_nothing() {
        let s = Surface::new(15, vec![]);
        assert_eq!(s.press(7, 0.0), None); // no slots: no API call
    }

    #[test]
    fn a_press_on_a_session_focuses_it() {
        let mut s = Surface::new(15, vec![]);
        s.slots = s.layout(&[row("Y:/projects-software/gem-trip", json!({"sessionId": "x"}))], &by_path());
        let (path, payload) = s.press(0, 0.0).unwrap();
        assert_eq!(path, "/api/focus");
        assert_eq!(payload, json!({"project": "gem-trip", "session_id": "x"}));
    }

    #[test]
    fn worktree_session_shares_its_projects_key() {
        let wt = "Y:/projects-software/gem-trip/.claude/worktrees/lark";
        let mut paths = by_path();
        paths.insert(crate::paths::abs_normcase(wt).unwrap(), "lark".into());
        let s = Surface::new(15, pinned(&["gem-trip"]));
        let slots = s.layout(&[row("Y:/projects-software/gem-trip", json!({"startedAt": 1})),
                               row(wt, json!({"startedAt": 2, "project": "gem-trip", "worktree": "lark"}))], &paths);
        assert_eq!(names(&slots), ["gem-trip", "gem-trip"]);
        assert_eq!(slots.iter().map(|sl| sl.count.clone()).collect::<Vec<_>>(),
                   [Some("1/2".to_string()), Some("2/2".to_string())]);
    }

    // -- Flash --------------------------------------------------------------

    #[test]
    fn first_sight_does_not_flash() {
        let mut s = Surface::new(15, vec![]);
        assert_eq!(s.flash_state("a", "idle", 0.0), "idle");
        assert!(!s.flashing());
    }

    #[test]
    fn a_change_alternates_new_and_old_then_settles() {
        let mut s = Surface::new(15, vec![]);
        s.flash_state("a", "working", 0.0);
        let seen: Vec<String> = [10.0, 10.3, 10.6, 11.1, 14.9].iter().map(|t| s.flash_state("a", "idle", *t)).collect();
        assert_eq!(seen, ["idle", "idle", "working", "idle", "working"]);
        assert!(s.flashing());
        assert_eq!(s.flash_state("a", "idle", 15.0), "idle");
        assert!(!s.flashing());
    }

    #[test]
    fn a_change_mid_flash_restarts_from_the_shown_state() {
        let mut s = Surface::new(15, vec![]);
        s.flash_state("a", "working", 0.0);
        s.flash_state("a", "idle", 1.0);
        assert_eq!(s.flash_state("a", "blocked", 2.0), "blocked");
        assert_eq!(s.flash_state("a", "blocked", 2.6), "idle");
    }

    // -- Decisions ----------------------------------------------------------

    fn pending() -> Vec<Value> {
        vec![json!({"id": "a1", "tool": "Bash", "cwd": "Y:/p/gem-trip"}),
             json!({"id": "b2", "tool": "Edit", "cwd": "Y:/p/amarantha"})]
    }

    #[test]
    fn no_pending_leaves_every_key_to_projects() {
        let mut s = Surface::new(15, vec![]);
        s.set_decision(&[], 0.0);
        assert_eq!(s.project_keys(), 15);
        assert_eq!(s.decision_slot(14), None);
    }

    #[test]
    fn pending_takes_the_last_three_keys() {
        let mut s = Surface::new(15, vec![]);
        s.set_decision(&pending(), 100.0);
        assert_eq!(s.project_keys(), 12);
        let slots: Vec<_> = (11..=14).map(|k| s.decision_slot(k)).collect();
        assert_eq!(slots, [None, Some("info"), Some("allow"), Some("deny")]);
        assert_eq!(s.decision.as_ref().unwrap()["id"], "a1"); // oldest first
    }

    #[test]
    fn a_press_before_the_key_is_armed_does_nothing() {
        let mut s = Surface::new(15, vec![]);
        s.set_decision(&pending(), 100.0);
        assert_eq!(s.decide_press(13, 100.2), None);
        assert_eq!(s.decide_press(13, 101.5), Some(json!({"id": "a1", "verdict": "allow", "who": "streamdeck"})));
        assert_eq!(s.decide_press(14, 101.5).unwrap()["verdict"], "deny");
    }

    #[test]
    fn the_info_key_never_decides() {
        let mut s = Surface::new(15, vec![]);
        s.set_decision(&pending(), 100.0);
        assert_eq!(s.decide_press(12, 200.0), None);
    }

    #[test]
    fn a_new_request_rearms_the_keys() {
        // The next request replacing the answered one must not inherit the
        // old arm time, or a double press would answer both.
        let mut s = Surface::new(15, vec![]);
        s.set_decision(&pending(), 100.0);
        s.set_decision(&pending()[1..], 105.0);
        assert_eq!(s.decide_press(13, 105.3), None);
        assert_eq!(s.decide_press(13, 106.5).unwrap()["id"], "b2");
    }

    #[test]
    fn the_same_request_keeps_its_arm_time() {
        let mut s = Surface::new(15, vec![]);
        s.set_decision(&pending(), 100.0);
        s.set_decision(&pending(), 103.0);
        assert!(s.decide_press(13, 103.1).is_some());
    }

    #[test]
    fn layout_shrinks_while_a_decision_shows() {
        let many: Vec<String> = (0..15).map(|i| format!("p{i}")).collect();
        let mut s = Surface::new(15, many);
        s.set_decision(&pending(), 0.0);
        assert_eq!(s.layout(&[], &HashMap::new()).len(), 12);
    }

    // -- The desktop-app glyph ----------------------------------------------

    #[test]
    fn glyph_marks_the_top_left_corner_only() {
        let fonts = Fonts { font: None };
        let look: Look = (String::new(), "working".into(), None, true, None);
        let img = render(&fonts, (72, 72), &look).to_rgb8();
        let (bg, fg) = colours("working");
        assert_eq!(img.get_pixel(5, 5).0, fg);
        assert_eq!(img.get_pixel(60, 60).0, bg);
        // Without the flag the corner is plain background.
        let plain: Look = (String::new(), "working".into(), None, false, None);
        assert_eq!(render(&fonts, (72, 72), &plain).to_rgb8().get_pixel(5, 5).0, bg);
    }
}
