#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{engine::general_purpose::STANDARD, Engine};
use percent_encoding::percent_decode_str;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::Duration,
};
use tiny_http::{Header, Method, Response, Server, StatusCode};

mod remote;

const ADDRESS: &str = "127.0.0.1:17873";
const HEIGHTMAP_N: usize = 256;
const APP_HTML: &str = include_str!("../7DtD_Skill_Tracker.html");
const REFDATA: &str = include_str!("refdata.json");
const UIASSETS: &str = include_str!("uiassets.json");

/// Encoding order of the 150 sandbox options (verified against real saves).
/// The SandboxCode triplet `[hi][mid][lo]` encodes option = hi*26+mid as the
/// index INTO THIS LIST, value = lo. Order must never change.
const SANDORDER: [&str; 150] = [
    "RangedDamage","MeleeDamage","BlockDamage","TerrainDamage","HeadshotMultiplier","CrouchSpeed","WalkSpeed","RunSpeed","JumpStrength","StaminaUsage","StaminaRegen","PlayerLevelBonusApplied","JarRefund","ShowHealthBars","ShowEnemyDamage","NewbieCoat","HeadshotMode","IncomingDamage","XPMultiplier","ShowXP","EncumbranceModifier","ItemDegradation","LoseItemsOnDeathType","LoseItemsOnDeathCount","DegradeItemsOnDeath","DegradeAmountOnDeath","DeathPenalty","DropOnDeath","DropOnQuit","InfectionRate","EnemySpawnMode","EntityDamage","BlockDamageAI","BlockDamageAIBM","ZombieMove","ZombieMoveNight","ZombieFeralMove","ZombieBMMove","ZombieFeralSense","AISmellMode","AllowZombieDigging","ZombieRageChance","EntityIncomingDamage","MaxEnemyTier","BiomeZombieRespawn","BiomeAnimalRespawn","BiomeEnemyDensity","ZombiesEatAnimals","BloodMoonFrequency","BloodMoonRange","BloodMoonWarning","BloodMoonEnemyCount","AirDropFrequency","AirDropMarker","AirDropRandomTime","BiomeProgression","TemperatureSurvival","StormFreq","StormWarning","HeatMapSensitivity","GlobalGSModifier","BiomeGSModifier","GlobalLSModifier","BiomeLSModifier","POITierLSModifier","GlobalTSModifier","DayNightLength","DayLightLength","AllowMap","AllowCompass","AllowScreenMarkers","ShowLocationInfo","ShowDayTime","WorkstationsInTheWild","MaxTechType","LootRespawnDays","LootTimer","LootMaxTier","GlobalLootCount","FoodLootCount","DrinkLootCount","MedicalLootCount","AmmoLootCount","ResourceLootCount","ArmorLootCount","MeleeLootCount","RangedLootCount","DukesLootCount","CraftingMagazinesLootCount","TreasureMapChance","LootBagChance","CropOutput","SeedDropOutput","CropGrowthSpeed","BackpackCrafting","WorkstationCrafting","CraftingProgression","CraftingTime","CraftingInput","CraftingOutput","CraftingMaxTier","MiningOutput","HarvestingOutput","ScrappingOutput","SmeltingType","DewCollectorTime","DewCollectorOutput","DewCollectorInput","ApiaryTime","ApiaryOutput","ApiaryInput","RepairTypes","MaxDegradationAmount","PointsPerMagazine","SkillGainRate","SkillPointsPerLevel","QuestsEnabled","IntroQuestEnabled","TraderToTraderQuestsEnabled","StarterSkillPoints","QuestsPerTier","QuestProgressionDailyLimit","BuriedQuestsEnabled","POIQuestsEnabled","TraderDialog","TraderHours","TradersEnabled","VendingEnabled","TraderSellPrices","TraderBuyPrices","TraderProtection","TraderResetInterval","TraderItemAbundance","TraderBuyLimit","TraderMaxTier","VendingResetInterval","VendingItemAbundance","ChallengesEnabled","IntroChallengesEnabled","VehicleFuelUsage","VehicleEntityDamage","VehicleBlockDamage","VehicleSelfDamage","ElectricalOutput","SillyCelebrate","SillyBigHeads","SillyTinyZombies","SillySounds","SillyLowGravity","SillyBlackandWhite",
];

#[derive(Clone, Serialize, Deserialize)]
struct Player {
    name: String,
    steam: String,
    login: String,
    pos: String,
    coop: bool,
    level: i32,
    progression: BTreeMap<String, i32>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Save {
    id: String,
    world: String,
    save: String,
    settings: Map<String, Value>,
    pl: Vec<Player>,
    scanned: bool,
    #[serde(rename = "hasMap")]
    has_map: bool,
    day: Option<i32>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Poi {
    name: String,
    x: i32,
    z: i32,
    y: i32,
    tier: i32,
    rotation: i32,
    width: i32,
    depth: i32,
    height: i32,
    thumbnail: Option<String>,
    zombies: i32,
    quests: String,
    theme: String,
    ingame: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct HeightMap {
    n: usize,
    mn: u8,
    mx: u8,
    d: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct WaterMask {
    n: usize,
    d: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct WorldMap {
    world: String,
    key: String,
    img: String,
    roads: Option<String>,
    water: Option<String>,
    size: i32,
    seed: String,
    ver: String,
    rwg: String,
    gen: BTreeMap<String, String>,
    pois: Vec<Poi>,
    hm: Option<HeightMap>,
    #[serde(rename = "watermask")]
    water_mask: Option<WaterMask>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ScanData {
    saves: Vec<Save>,
    maps: BTreeMap<String, WorldMap>,
    generated_at: String,
}

#[derive(Clone)]
struct Paths {
    appdata: PathBuf,
    install: Option<PathBuf>,
}

/// A remote server connection (filled once the user enters credentials).
/// Lives ONLY in memory for the session — never written to disk, never sent to
/// the browser (the UI keeps host/user/port/base; the password stays here).
#[derive(Clone)]
struct RemoteConn {
    proto: String, // "sftp" | "ftp" | "ftps"
    host: String,
    port: u16,
    user: String,
    pass: String,
    base: String, // remote 7DaysToDie data dir containing Saves/ + GeneratedWorlds/
}

/// The active data source. Default = the local game folder. The user can repoint
/// it at a custom/UNC(SMB) path, or at a temp cache populated by a remote download.
struct Source {
    root: PathBuf,
    kind: String, // "local" | "smb" | "sftp" | "ftp" | "ftps" | "share"
    label: String,
    conn: Option<RemoteConn>,
}

impl Source {
    fn remote_cache(&self) -> bool {
        self.kind == "sftp" || self.kind == "ftp" || self.kind == "ftps" || self.kind == "share"
    }
}

/// On bind failure (port already in use) decide whether the squatter is OUR already-
/// running instance (then just open the browser to it) or a FOREIGN service (then warn
/// instead of opening the browser to someone else's local page). Best-effort, short timeout.
fn port_is_our_instance() -> bool {
    use std::io::{Read as _, Write as _};
    let mut stream = match std::net::TcpStream::connect(ADDRESS) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(800)));
    if stream
        .write_all(b"GET /api/health HTTP/1.1\r\nHost: 127.0.0.1:17873\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buf = String::new();
    let _ = stream.take(4096).read_to_string(&mut buf);
    buf.contains("\"ok\":true") || buf.contains("\"ok\": true")
}

fn main() {
    let appdata = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("7DaysToDie");
    let paths = Paths {
        appdata,
        install: find_install(),
    };

    let server = match Server::http(ADDRESS) {
        Ok(server) => server,
        Err(error) => {
            // Port busy. If it's our own already-running instance, just open the browser
            // to it; if it's a FOREIGN service (or nothing answers), don't hand the user
            // someone else's page — warn instead.
            if port_is_our_instance() {
                open_browser(&launch_url());
            } else {
                eprintln!("[7DtD] Port {ADDRESS} ist belegt oder Start fehlgeschlagen ({error}) — Browser NICHT geöffnet.");
            }
            return;
        }
    };
    let cache: Arc<Mutex<Option<ScanData>>> = Arc::new(Mutex::new(None));
    let source: Arc<Mutex<Source>> = Arc::new(Mutex::new(Source {
        root: paths.appdata.clone(),
        kind: "local".into(),
        label: "Local game saves".into(),
        conn: None,
    }));
    println!("[7DtD] Rust Companion läuft: http://{ADDRESS}");

    thread::spawn(|| {
        thread::sleep(Duration::from_millis(350));
        open_browser(&launch_url());
    });

    // Worker pool: a slow /api/scan must not freeze asset/write/health requests.
    let server = Arc::new(server);
    let mut workers = Vec::new();
    for _ in 0..8 {
        let server = server.clone();
        let paths = paths.clone();
        let cache = cache.clone();
        let source = source.clone();
        workers.push(thread::spawn(move || {
            while let Ok(request) = server.recv() {
                if let Err(error) = handle(request, &paths, &cache, &source) {
                    eprintln!("[7DtD] Request-Fehler: {error}");
                }
            }
        }));
    }
    for worker in workers {
        let _ = worker.join();
    }
}

fn req_header<'a>(request: &'a tiny_http::Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// Reject cross-origin / rebinding requests. The server binds 127.0.0.1, but a
/// website the user visits (or a DNS-rebinding attacker) could otherwise POST to
/// our write endpoints and corrupt saves. The Host check is the load-bearing
/// defence against rebinding (the attacker's request carries the attacker host);
/// the Origin check blocks cross-site fetches. Same-origin navigations omit Origin.
fn local_request_ok(request: &tiny_http::Request) -> bool {
    let host_ok = matches!(
        req_header(request, "Host"),
        Some(h) if h == ADDRESS || h == "localhost:17873" || h == "[::1]:17873"
    );
    let origin_ok = match req_header(request, "Origin") {
        None => true,
        Some(o) => {
            o == "http://127.0.0.1:17873"
                || o == "http://localhost:17873"
                || o == "http://[::1]:17873"
        }
    };
    host_ok && origin_ok
}

/// Hard ceiling on any request body (64 MiB). The server is loopback-only, but a
/// buggy/malicious same-origin tab could otherwise stream an unbounded body and
/// exhaust memory (8-thread pool). Rejects an oversized declared Content-Length up
/// front and caps the actual read so a lying/absent length still can't over-allocate.
const MAX_BODY: u64 = 64 * 1024 * 1024;
fn read_body_limited(request: &mut tiny_http::Request) -> Result<String, String> {
    if let Some(len) = req_header(request, "Content-Length").and_then(|v| v.parse::<u64>().ok()) {
        if len > MAX_BODY {
            return Err(format!("Body zu groß ({len} Bytes, Limit {MAX_BODY})"));
        }
    }
    let mut body = String::new();
    request
        .as_reader()
        .take(MAX_BODY)
        .read_to_string(&mut body)
        .map_err(|e| format!("Body unlesbar: {e}"))?;
    Ok(body)
}

/// The active paths: install is always local, but the data root follows the
/// selected source (local / custom / SMB / remote cache).
fn eff_paths(paths: &Paths, source: &Arc<Mutex<Source>>) -> Paths {
    let root = source
        .lock()
        .ok()
        .map(|s| s.root.clone())
        .unwrap_or_else(|| paths.appdata.clone());
    Paths {
        appdata: root,
        install: paths.install.clone(),
    }
}

fn handle(
    mut request: tiny_http::Request,
    paths: &Paths,
    cache: &Arc<Mutex<Option<ScanData>>>,
    source: &Arc<Mutex<Source>>,
) -> Result<(), String> {
    let raw_url = request.url().to_string();
    let path = raw_url.split('?').next().unwrap_or("/").to_string();
    if !local_request_ok(&request) {
        return respond_status_json(
            request,
            StatusCode(403),
            &json!({"ok": false, "error": "Ungültiger Origin/Host"}),
        );
    }
    // Source switching (local/SMB path + remote SFTP/FTP). Body = JSON.
    if request.method() == &Method::Post && path.starts_with("/api/source/") {
        let body = match read_body_limited(&mut request) {
            Ok(b) => b,
            Err(e) => return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": e})),
        };
        let payload: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        return handle_source(request, paths, cache, source, &path, payload);
    }
    if request.method() == &Method::Get && path == "/api/source" {
        let info = {
            let s = source.lock().map_err(|e| e.to_string())?;
            json!({
                "kind": s.kind, "label": s.label, "root": s.root.display().to_string(),
                "remote": s.remote_cache(), "canWrite": !s.remote_cache(),
            })
        };
        return respond_json(request, &info);
    }
    // World sharing: export a bundle of the active world, or import a friend's bundle.
    if request.method() == &Method::Get && path == "/api/share/export" {
        let eff = eff_paths(paths, source);
        return match build_share_bundle(&eff) {
            Ok(bundle) => {
                let world = bundle.get("world").and_then(|v| v.as_str()).unwrap_or("world").to_string();
                serve_share_download(request, &bundle, &world)
            }
            Err(e) => respond_status_json(request, StatusCode(500), &json!({"ok": false, "error": e})),
        };
    }
    if request.method() == &Method::Post && path == "/api/share/import" {
        let body = match read_body_limited(&mut request) {
            Ok(b) => b,
            Err(e) => return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": e})),
        };
        let bundle: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": format!("JSON ungültig: {e}")})),
        };
        return import_share(request, cache, source, bundle);
    }
    if request.method() == &Method::Post && path == "/api/write-settings" {
        if !req_header(&request, "Content-Type")
            .map(|c| c.starts_with("application/json"))
            .unwrap_or(false)
        {
            return respond_status_json(
                request,
                StatusCode(415),
                &json!({"ok": false, "error": "Content-Type muss application/json sein"}),
            );
        }
        let body = match read_body_limited(&mut request) {
            Ok(b) => b,
            Err(e) => {
                return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": e}))
            }
        };
        // Resolve the read-only check AND the write root under ONE lock so a concurrent
        // /api/source/* swap can't slip between the guard and the path resolution (TOCTOU).
        let eff = {
            let s = match source.lock() {
                Ok(s) => s,
                Err(_) => return respond_status_json(request, StatusCode(500), &json!({"ok": false, "error": "Source-Lock vergiftet"})),
            };
            if s.remote_cache() {
                return respond_status_json(
                    request,
                    StatusCode(409),
                    &json!({"ok": false, "error": "Write-back is read-only for remote (SFTP/FTP) sources. Use local or a mounted SMB share to edit settings."}),
                );
            }
            Paths { appdata: s.root.clone(), install: paths.install.clone() }
        };
        return match write_settings(&eff, &body) {
            Ok(value) => respond_json(request, &value),
            Err(error) => {
                respond_status_json(request, StatusCode(500), &json!({"ok": false, "error": error}))
            }
        };
    }
    if request.method() == &Method::Post && path == "/api/restore-settings" {
        if !req_header(&request, "Content-Type")
            .map(|c| c.starts_with("application/json"))
            .unwrap_or(false)
        {
            return respond_status_json(
                request,
                StatusCode(415),
                &json!({"ok": false, "error": "Content-Type muss application/json sein"}),
            );
        }
        let body = match read_body_limited(&mut request) {
            Ok(b) => b,
            Err(e) => {
                return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": e}))
            }
        };
        // One lock for the read-only check + write root (TOCTOU, see write-settings).
        let eff = {
            let s = match source.lock() {
                Ok(s) => s,
                Err(_) => return respond_status_json(request, StatusCode(500), &json!({"ok": false, "error": "Source-Lock vergiftet"})),
            };
            if s.remote_cache() {
                return respond_status_json(
                    request,
                    StatusCode(409),
                    &json!({"ok": false, "error": "Restore is read-only for remote (SFTP/FTP) sources."}),
                );
            }
            Paths { appdata: s.root.clone(), install: paths.install.clone() }
        };
        return match restore_settings(&eff, &body) {
            Ok(value) => respond_json(request, &value),
            Err(error) => {
                respond_status_json(request, StatusCode(500), &json!({"ok": false, "error": error}))
            }
        };
    }
    match (request.method(), path.as_str()) {
        (&Method::Get, "/") | (&Method::Get, "/index.html") => serve_bytes(
            request,
            APP_HTML.as_bytes().to_vec(),
            "text/html; charset=utf-8",
        ),
        (&Method::Get, "/api/health") => respond_json(request, &json!({"ok": true})),
        (&Method::Get, "/api/refdata") => serve_bytes(
            request,
            REFDATA.as_bytes().to_vec(),
            "application/json; charset=utf-8",
        ),
        (&Method::Get, "/api/uiassets") => serve_bytes(
            request,
            UIASSETS.as_bytes().to_vec(),
            "application/json; charset=utf-8",
        ),
        (&Method::Get, "/api/scan") | (&Method::Post, "/api/scan") => {
            // imported share: no raw saves to re-scan — serve the bundled scan
            if source.lock().map(|s| s.kind == "share").unwrap_or(false) {
                if let Some(d) = cache.lock().map_err(|e| e.to_string())?.clone() {
                    return respond_json(request, &d);
                }
            }
            match scan(&eff_paths(paths, source)) {
                Ok(data) => {
                    *cache.lock().map_err(|e| e.to_string())? = Some(data.clone());
                    respond_json(request, &data)
                }
                Err(error) => respond_status_json(
                    request,
                    StatusCode(500),
                    &json!({"ok": false, "error": error}),
                ),
            }
        }
        // Lightweight live refresh: re-reads ONLY the saves (settings + players/.ttp + world day),
        // never the maps/POIs/heightmaps. Cheap enough to poll every few seconds while the game runs.
        (&Method::Get, "/api/refresh") => {
            if source.lock().map(|s| s.kind == "share").unwrap_or(false) {
                let saves = cache
                    .lock()
                    .map_err(|e| e.to_string())?
                    .as_ref()
                    .map(|d| d.saves.clone())
                    .unwrap_or_default();
                return respond_json(request, &json!({ "saves": saves }));
            }
            match scan_saves(&eff_paths(paths, source)) {
                Ok(saves) => respond_json(request, &json!({ "saves": saves })),
                Err(error) => respond_status_json(
                    request,
                    StatusCode(500),
                    &json!({ "ok": false, "error": error }),
                ),
            }
        }
        (&Method::Get, "/api/data") => {
            let current = cache.lock().map_err(|e| e.to_string())?.clone();
            match current {
                Some(data) => respond_json(request, &data),
                None => respond_status_json(
                    request,
                    StatusCode(404),
                    &json!({"ok": false, "error": "Noch nicht gescannt"}),
                ),
            }
        }
        _ if path.starts_with("/world/") => serve_world_asset(request, &eff_paths(paths, source), &path),
        _ if path.starts_with("/poi/") => serve_poi_asset(request, paths, &path),
        _ => respond_status_json(
            request,
            StatusCode(404),
            &json!({"ok": false, "error": "Nicht gefunden"}),
        ),
    }
}

fn scan_and_respond(
    request: tiny_http::Request,
    paths: &Paths,
    cache: &Arc<Mutex<Option<ScanData>>>,
    source: &Arc<Mutex<Source>>,
) -> Result<(), String> {
    let eff = eff_paths(paths, source);
    match scan(&eff) {
        Ok(data) => {
            *cache.lock().map_err(|e| e.to_string())? = Some(data.clone());
            respond_json(request, &data)
        }
        Err(error) => {
            respond_status_json(request, StatusCode(500), &json!({"ok": false, "error": error}))
        }
    }
}

/// Source switching: local/custom/SMB path (Phase 0) and remote SFTP/FTP (Phase 1/2).
fn handle_source(
    request: tiny_http::Request,
    paths: &Paths,
    cache: &Arc<Mutex<Option<ScanData>>>,
    source: &Arc<Mutex<Source>>,
    path: &str,
    body: Value,
) -> Result<(), String> {
    match path {
        "/api/source/local" => {
            let p = body
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if p.is_empty() {
                return respond_status_json(
                    request,
                    StatusCode(400),
                    &json!({"ok": false, "error": "Pfad fehlt"}),
                );
            }
            let pb = PathBuf::from(&p);
            if !pb.join("Saves").is_dir() {
                return respond_status_json(
                    request,
                    StatusCode(400),
                    &json!({"ok": false, "error": format!("Kein 'Saves'-Ordner in {p}. Zeig auf den 7DaysToDie-Daten-Ordner (enthält Saves und GeneratedWorlds).")}),
                );
            }
            let kind = if p.starts_with("\\\\") { "smb" } else { "local" };
            {
                let mut s = source.lock().map_err(|e| e.to_string())?;
                s.root = pb;
                s.kind = kind.into();
                s.label = p;
                s.conn = None;
            }
            *cache.lock().map_err(|e| e.to_string())? = None;
            scan_and_respond(request, paths, cache, source)
        }
        "/api/source/reset" => {
            {
                let mut s = source.lock().map_err(|e| e.to_string())?;
                s.root = paths.appdata.clone();
                s.kind = "local".into();
                s.label = "Local game saves".into();
                s.conn = None;
            }
            *cache.lock().map_err(|e| e.to_string())? = None;
            scan_and_respond(request, paths, cache, source)
        }
        "/api/source/test" | "/api/source/remote-list" | "/api/source/remote-fetch" => {
            handle_remote(request, paths, cache, source, path, body)
        }
        _ => respond_status_json(
            request,
            StatusCode(404),
            &json!({"ok": false, "error": "Unbekannte Source-Route"}),
        ),
    }
}

fn remote_cache_dir() -> PathBuf {
    std::env::temp_dir().join("7dtd-companion-remote")
}

/// Serializes remote fetches so two concurrent downloads can't race on the shared
/// cache dir (one wiping it while the other writes). Single-user app, so a global
/// lock is fine — fetches are rare and never need to overlap.
fn fetch_guard() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

fn parse_conn(body: &Value) -> Result<RemoteConn, String> {
    let proto = body
        .get("proto")
        .and_then(|v| v.as_str())
        .unwrap_or("sftp")
        .to_lowercase();
    let host = body.get("host").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let default_port = if proto == "ftp" || proto == "ftps" { 21 } else { 22 };
    let port = body.get("port").and_then(|v| v.as_u64()).unwrap_or(default_port) as u16;
    let user = body.get("user").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let pass = body.get("pass").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let base = body.get("base").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if host.is_empty() {
        return Err("Host fehlt".into());
    }
    if base.is_empty() {
        return Err("Basis-Pfad fehlt (der 7DaysToDie-Ordner auf dem Server, enthält Saves/ + GeneratedWorlds/)".into());
    }
    Ok(RemoteConn { proto, host, port, user, pass, base })
}

// Remote (SFTP/FTP) source handling.
fn handle_remote(
    request: tiny_http::Request,
    paths: &Paths,
    cache: &Arc<Mutex<Option<ScanData>>>,
    source: &Arc<Mutex<Source>>,
    path: &str,
    body: Value,
) -> Result<(), String> {
    match path {
        "/api/source/test" => {
            let conn = match parse_conn(&body) {
                Ok(c) => c,
                Err(e) => return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": e})),
            };
            match remote::test(&conn) {
                Ok(v) => respond_json(request, &v),
                Err(e) => respond_status_json(request, StatusCode(502), &json!({"ok": false, "error": e})),
            }
        }
        "/api/source/remote-list" => {
            let conn = match parse_conn(&body) {
                Ok(c) => c,
                Err(e) => return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": e})),
            };
            match remote::list(&conn) {
                Ok(v) => {
                    if let Ok(mut s) = source.lock() {
                        s.conn = Some(conn);
                    }
                    respond_json(request, &v)
                }
                Err(e) => respond_status_json(request, StatusCode(502), &json!({"ok": false, "error": e})),
            }
        }
        "/api/source/remote-fetch" => {
            // serialize concurrent fetches (shared cache dir). try_lock so a second
            // fetch returns 409 immediately instead of silently pinning a worker thread
            // for the whole in-flight download.
            let _fetch_g = match fetch_guard().try_lock() {
                Ok(g) => g,
                Err(std::sync::TryLockError::WouldBlock) => {
                    return respond_status_json(request, StatusCode(409), &json!({"ok": false, "error": "Ein Fetch läuft bereits — bitte warten."}))
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return respond_status_json(request, StatusCode(500), &json!({"ok": false, "error": "fetch lock vergiftet"}))
                }
            };
            let world = body.get("world").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let save = body.get("save").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if world.is_empty() || save.is_empty() {
                return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": "world/save fehlt"}));
            }
            // Defence in depth: reject path-traversal in the user-supplied world/save
            // before they reach the remote-fetch cache write path (remote.rs validates too).
            if safe_segment(&world).is_err() || safe_segment(&save).is_err() {
                return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": "Ungültiger Welt-/Save-Name"}));
            }
            let conn = {
                let stored = source.lock().ok().and_then(|s| s.conn.clone());
                match stored.or_else(|| parse_conn(&body).ok()) {
                    Some(c) => c,
                    None => return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": "Erst verbinden (Verbindung testen/auflisten)."})),
                }
            };
            let cdir = remote_cache_dir();
            let _ = std::fs::remove_dir_all(&cdir);
            if let Err(e) = std::fs::create_dir_all(&cdir) {
                return respond_status_json(request, StatusCode(500), &json!({"ok": false, "error": format!("Cache-Ordner: {e}")}));
            }
            match remote::fetch(&conn, &world, &save, &cdir) {
                Ok(_) => {
                    {
                        let mut s = source.lock().map_err(|e| e.to_string())?;
                        s.root = cdir;
                        s.kind = conn.proto.clone();
                        s.label = format!("{}://{}/{}/{}", conn.proto, conn.host, world, save);
                        s.conn = Some(conn);
                    }
                    *cache.lock().map_err(|e| e.to_string())? = None;
                    scan_and_respond(request, paths, cache, source)
                }
                Err(e) => respond_status_json(request, StatusCode(502), &json!({"ok": false, "error": e})),
            }
        }
        _ => respond_status_json(request, StatusCode(404), &json!({"ok": false, "error": "Unbekannte Remote-Route"})),
    }
}

// Texture files the 3D map needs (the heightmap is already baked into the scan JSON
// as a small 256² sample, so the heavy dtm.raw is NOT shared — keeps the bundle ~4 MB).
const SHARE_ASSETS: &[&str] = &["biomes.png", "splat3_half.png", "splat4_half.png"];

/// Build a self-contained "world share" bundle from the active source: the full scan
/// JSON (saves + maps incl. heightmap/water/POIs) plus the map texture files (base64).
/// A friend imports this and gets the whole 3D map without owning the world files.
fn build_share_bundle(paths: &Paths) -> Result<Value, String> {
    let mut data = scan(paths)?;
    // Privacy: a share bundle is sent to OTHER people. Strip the per-player PII the recipient
    // never needs for the map — Steam id, last-login time and world POSITION (base coordinates) —
    // so sharing a world never doxes you or co-op partners (critical on an 8-player PvP server).
    for s in data.saves.iter_mut() {
        for p in s.pl.iter_mut() {
            p.steam.clear();
            p.login.clear();
            p.pos.clear();
        }
    }
    let worlds_root = paths.appdata.join("GeneratedWorlds");
    let mut assets = Map::new();
    // Cap the in-memory bundle so many/large worlds can't OOM the process (every
    // texture is read fully and base64-encoded into the JSON before sending).
    const MAX_BUNDLE: usize = 256 * 1024 * 1024;
    let mut total = 0usize;
    for world in data.maps.keys() {
        let dir = worlds_root.join(world);
        for f in SHARE_ASSETS {
            if let Ok(bytes) = fs::read(dir.join(f)) {
                total += bytes.len();
                if total > MAX_BUNDLE {
                    return Err(format!(
                        "Welt-Texturen überschreiten {MAX_BUNDLE} Bytes — Bundle zu groß zum Teilen."
                    ));
                }
                assets.insert(format!("{world}/{f}"), Value::String(STANDARD.encode(bytes)));
            }
        }
    }
    let world = data.maps.keys().next().cloned().unwrap_or_else(|| "world".into());
    let scan_value = serde_json::to_value(&data).map_err(|e| e.to_string())?;
    Ok(json!({
        "kind": "7dtd-world-share", "v": 1, "world": world,
        "scan": scan_value, "assets": Value::Object(assets),
    }))
}

fn serve_share_download(request: tiny_http::Request, bundle: &Value, world: &str) -> Result<(), String> {
    let body = serde_json::to_vec(bundle).map_err(|e| e.to_string())?;
    let safe: String = world.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    let fname = format!("{safe}.7dtdworld.json");
    let resp = Response::from_data(body)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap())
        .with_header(
            Header::from_bytes(&b"Content-Disposition"[..], format!("attachment; filename=\"{fname}\"").as_bytes()).unwrap(),
        );
    request.respond(resp).map_err(|e| e.to_string())
}

/// Import a world-share bundle: write its textures into the cache, mark the source as
/// a read-only "share", and serve the bundled scan directly (the cache has no raw saves
/// to re-scan, so the scan route returns this cached copy while kind == "share").
fn import_share(
    request: tiny_http::Request,
    cache: &Arc<Mutex<Option<ScanData>>>,
    source: &Arc<Mutex<Source>>,
    bundle: Value,
) -> Result<(), String> {
    if bundle.get("kind").and_then(|v| v.as_str()) != Some("7dtd-world-share") {
        return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": "Keine gültige World-Share-Datei"}));
    }
    let scan_value = match bundle.get("scan").cloned() {
        Some(v) => v,
        None => return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": "Share-Datei ohne Scan-Daten"})),
    };
    let data: ScanData = match serde_json::from_value(scan_value) {
        Ok(d) => d,
        Err(e) => return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": format!("Scan-Daten unlesbar: {e}")})),
    };
    let cdir = remote_cache_dir();
    let _ = fs::remove_dir_all(&cdir);
    if let Err(e) = fs::create_dir_all(&cdir) {
        return respond_status_json(request, StatusCode(500), &json!({"ok": false, "error": format!("Cache-Ordner: {e}")}));
    }
    if let Some(assets) = bundle.get("assets").and_then(|v| v.as_object()) {
        // Import-side caps mirror the export MAX_BUNDLE: a hostile bundle must not write an
        // unbounded number/size of files into the temp cache (disk-fill DoS on mere import).
        const MAX_IMPORT_FILES: usize = 512;
        const MAX_IMPORT_BYTES: usize = 256 * 1024 * 1024;
        if assets.len() > MAX_IMPORT_FILES {
            return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": "Share-Datei hat zu viele Asset-Dateien"}));
        }
        let mut total = 0usize;
        for (k, v) in assets {
            let b64 = match v.as_str() { Some(s) => s, None => continue };
            let (w, f) = match k.rsplit_once('/') { Some(x) => x, None => continue };
            // path-traversal guard: BOTH the world dir and the file must be a single safe
            // segment (the world part was previously only checked for "..", letting an
            // absolute/drive-qualified `w` like `C:\Windows\x` escape the cache root).
            if safe_segment(w).is_err() || safe_segment(f).is_err() {
                continue;
            }
            if let Ok(bytes) = STANDARD.decode(b64) {
                total += bytes.len();
                if total > MAX_IMPORT_BYTES {
                    return respond_status_json(request, StatusCode(400), &json!({"ok": false, "error": "Share-Datei zu groß (Asset-Daten überschreiten das Limit)"}));
                }
                let dir = cdir.join("GeneratedWorlds").join(w);
                if fs::create_dir_all(&dir).is_ok() {
                    let _ = fs::write(dir.join(f), bytes);
                }
            }
        }
    }
    let world = bundle.get("world").and_then(|v| v.as_str()).unwrap_or("world").to_string();
    {
        let mut s = source.lock().map_err(|e| e.to_string())?;
        s.root = cdir;
        s.kind = "share".into();
        s.label = format!("Shared world: {world}");
        s.conn = None;
    }
    *cache.lock().map_err(|e| e.to_string())? = Some(data.clone());
    respond_json(request, &data)
}

// Saves-only scan (no maps): used by /api/refresh for cheap live polling. Mirrors the saves loop in scan().
fn scan_saves(paths: &Paths) -> Result<Vec<Save>, String> {
    let saves_root = paths.appdata.join("Saves");
    let worlds_root = paths.appdata.join("GeneratedWorlds");
    if !saves_root.is_dir() {
        return Err(format!("Save-Ordner fehlt: {}", saves_root.display()));
    }
    let mut saves = Vec::new();
    for world in read_dirs(&saves_root)? {
        let world_dir = saves_root.join(&world);
        for save_name in read_dirs(&world_dir)? {
            let save_dir = world_dir.join(&save_name);
            let sdf = save_dir.join("gameOptions.sdf");
            if !sdf.is_file() {
                continue;
            }
            let settings = match fs::read(&sdf) {
                Ok(bytes) => parse_sdf(&bytes),
                Err(error) => {
                    eprintln!(
                        "[7DtD] gameOptions.sdf unlesbar, Save übersprungen ({}): {error}",
                        sdf.display()
                    );
                    continue;
                }
            };
            let players = parse_players(&save_dir);
            saves.push(Save {
                id: format!("{world} / {save_name}"),
                world: world.clone(),
                save: save_name,
                settings,
                pl: players,
                scanned: true,
                has_map: worlds_root.join(&world).is_dir(),
                day: read_world_day(&save_dir),
            });
        }
    }
    saves.sort_by_key(|a| a.id.to_lowercase());
    Ok(saves)
}

fn scan(paths: &Paths) -> Result<ScanData, String> {
    let saves_root = paths.appdata.join("Saves");
    let worlds_root = paths.appdata.join("GeneratedWorlds");
    if !saves_root.is_dir() {
        return Err(format!("Save-Ordner fehlt: {}", saves_root.display()));
    }

    let mut world_names = Vec::new();
    if worlds_root.is_dir() {
        for entry in read_dirs(&worlds_root)? {
            world_names.push(entry);
        }
    }

    let mut needed = HashSet::new();
    for world in &world_names {
        let prefab_file = worlds_root.join(world).join("prefabs.xml");
        if let Ok(text) = fs::read_to_string(prefab_file) {
            for name in placed_prefab_names(&text) {
                needed.insert(name);
            }
        }
    }
    let prefab_meta = load_prefab_meta(paths.install.as_deref(), &needed);
    let poi_names = load_poi_names(paths.install.as_deref());

    let mut saves = Vec::new();
    for world in read_dirs(&saves_root)? {
        let world_dir = saves_root.join(&world);
        for save_name in read_dirs(&world_dir)? {
            let save_dir = world_dir.join(&save_name);
            let sdf = save_dir.join("gameOptions.sdf");
            if !sdf.is_file() {
                continue;
            }
            let settings = match fs::read(&sdf) {
                Ok(bytes) => parse_sdf(&bytes),
                Err(error) => {
                    eprintln!("[7DtD] gameOptions.sdf unlesbar, Save übersprungen ({}): {error}", sdf.display());
                    continue;
                }
            };
            let players = parse_players(&save_dir);
            saves.push(Save {
                id: format!("{world} / {save_name}"),
                world: world.clone(),
                save: save_name,
                settings,
                pl: players,
                scanned: true,
                has_map: worlds_root.join(&world).is_dir(),
                day: read_world_day(&save_dir),
            });
        }
    }
    saves.sort_by_key(|a| a.id.to_lowercase());

    let mut maps = BTreeMap::new();
    for world in world_names {
        let dir = worlds_root.join(&world);
        let biomes = dir.join("biomes.png");
        let prefabs = dir.join("prefabs.xml");
        if !biomes.is_file() || !prefabs.is_file() {
            continue;
        }
        let map_info = parse_map_info(&dir.join("map_info.xml"));
        let prefab_text = fs::read_to_string(&prefabs).unwrap_or_default();
        let key = encode_segment(&world);
        let pois = parse_prefabs(&prefab_text, &prefab_meta, &poi_names);
        let dtm = dir.join("dtm.raw");
        let hm = if dtm.is_file() {
            sample_heightmap(&dtm, map_info.size as usize).ok()
        } else {
            None
        };
        // 2D water cannot come from a <canvas> (splat4 has alpha=0 → premultiplied to
        // black). Decode the blue channel server-side and max-pool it so thin rivers
        // survive the downsample.
        let splat4 = [dir.join("splat4_half.png"), dir.join("splat4.png")]
            .into_iter()
            .find(|p| p.is_file());
        let water_mask = splat4.and_then(|p| water_mask(&p, 1536));
        maps.insert(
            world.clone(),
            WorldMap {
                world: world.clone(),
                key: key.clone(),
                img: format!("/world/{key}/biomes.png"),
                // Prefer the smaller half-res splats (5120² instead of 10240²) — the
                // full-res overlays are ~100 MP and were the main browser-map lag source.
                roads: file_url_if_exists(&dir, &key, "splat3_half.png")
                    .or_else(|| file_url_if_exists(&dir, &key, "splat3.png")),
                water: file_url_if_exists(&dir, &key, "splat4_half.png")
                    .or_else(|| file_url_if_exists(&dir, &key, "splat4.png")),
                size: map_info.size,
                seed: map_info.seed,
                ver: map_info.ver,
                rwg: map_info.rwg,
                gen: map_info.gen,
                pois,
                hm,
                water_mask,
            },
        );
    }

    Ok(ScanData {
        saves,
        maps,
        generated_at: format!("{:?}", std::time::SystemTime::now()),
    })
}

fn read_dirs(path: &Path) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let entries = fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            result.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    result.sort_by_key(|s| s.to_lowercase());
    Ok(result)
}

fn find_install() -> Option<PathBuf> {
    let candidates = [
        r"D:\Steam\steamapps\common\7 Days To Die",
        r"C:\Program Files (x86)\Steam\steamapps\common\7 Days To Die",
        r"C:\SteamLibrary\steamapps\common\7 Days To Die",
        r"D:\SteamLibrary\steamapps\common\7 Days To Die",
        r"E:\SteamLibrary\steamapps\common\7 Days To Die",
    ];
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| path.join("Data/Prefabs/POIs").is_dir())
}

/// Cache-bust the launch URL with the binary's own modification time so a freshly
/// built/deployed exe opens a NEW browser tab (user always sees current code), while
/// relaunching the same exe reuses the existing tab (no tab spam). The HTTP router
/// strips the query (see `path` in `handle`), so `/?v=N` still serves "/".
fn launch_url() -> String {
    let v = std::env::current_exe()
        .and_then(std::fs::metadata)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("http://{ADDRESS}/?v={v}")
}

/// Open the default browser WITHOUT spawning cmd.exe. The `open` crate used
/// `cmd /c start <url>`, and a process that reads user files then spawns cmd to
/// reach the network is exactly the behaviour heuristic AV flags as a password
/// stealer (false positive). ShellExecuteW opens the shell association directly,
/// no child process.
#[cfg(windows)]
fn open_browser(url: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| {
        let mut v: Vec<u16> = s.encode_utf16().collect();
        v.push(0);
        v
    };
    let op = wide("open");
    let file = wide(url);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(not(windows))]
fn open_browser(_url: &str) {}

fn parse_sdf(bytes: &[u8]) -> Map<String, Value> {
    let mut out = Map::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let kind = bytes[pos];
        pos += 1;
        if pos + 3 > bytes.len() {
            break;
        }
        let key_len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
        pos += 3;
        if pos + key_len > bytes.len() {
            break;
        }
        let key = String::from_utf8_lossy(&bytes[pos..pos + key_len]).into_owned();
        pos += key_len;
        let value = match kind {
            1 if pos + 4 <= bytes.len() => {
                let value = i32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
                pos += 4;
                json!(value)
            }
            2 if pos + 3 <= bytes.len() => {
                let len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
                pos += 3;
                if pos + len > bytes.len() {
                    break;
                }
                let raw = &bytes[pos..pos + len];
                pos += len;
                let decoded = STANDARD
                    .decode(raw)
                    .ok()
                    .and_then(|v| String::from_utf8(v).ok())
                    .unwrap_or_else(|| String::from_utf8_lossy(raw).into_owned());
                json!(decoded)
            }
            3 if pos < bytes.len() => {
                let value = bytes[pos] != 0;
                pos += 1;
                json!(value)
            }
            4 if pos + 4 <= bytes.len() => {
                let value = f32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
                pos += 4;
                json!(value)
            }
            _ => break,
        };
        out.insert(key, value);
    }
    out
}

/// Current in-game day from main.ttw. worldTime is a uint64 in the header whose
/// position is version-dependent (follows WorldState.SaveLoad write order), so we
/// parse structurally rather than by fixed offset. day = worldTime / 24000 + 1.
fn read_world_day(save_dir: &Path) -> Option<i32> {
    let bytes = fs::read(save_dir.join("main.ttw")).ok()?;
    if bytes.len() < 8 || &bytes[0..4] != b"ttw\0" {
        return None;
    }
    let read_u32 = |p: usize| -> Option<u32> {
        bytes.get(p..p + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
    };
    let mut pos = 4usize;
    let version = read_u32(pos)?;
    pos += 4;
    if version > 11 {
        // length-prefixed game-version string (1-byte 7-bit length for short strings)
        let len = *bytes.get(pos)? as usize;
        pos += 1 + len;
    }
    if version > 14 {
        pos += 16; // releaseType, major, minor, build (4x int32)
    }
    if read_u32(pos)? != 0 {
        return None; // header layout drift — keep the user's own day, not a bogus one
    }
    pos += 4; // uint32 constant 0
    if version > 6 {
        pos += 4; // int32 activeGameMode
    }
    if read_u32(pos)? != 0 {
        return None; // header layout drift — keep the user's own day, not a bogus one
    }
    pos += 4; // uint32 constant 0
    pos += 4; // float waterLevel
    pos += 16; // chunkSizeX/Z/Y + chunkCount (4x int32)
    pos += 4; // int32 providerId
    pos += 4; // int32 seed
    let raw = bytes.get(pos..pos + 8)?;
    let world_time = u64::from_le_bytes(raw.try_into().unwrap());
    // Bounds guard: a future header change would land worldTime on arbitrary bytes
    // and yield an absurd day that the UI trusts over the user's input. Drop it
    // instead (None -> UI keeps its own day value).
    let day = (world_time / 24000) as i64 + 1;
    if !(1..=100000).contains(&day) {
        return None;
    }
    Some(day as i32)
}

fn parse_players(save_dir: &Path) -> Vec<Player> {
    let xml = fs::read_to_string(save_dir.join("players.xml")).unwrap_or_default();
    let player_re = Regex::new(r#"<player\b([^>]*)>"#).unwrap();
    let attr_re = Regex::new(r#"([A-Za-z]+)="([^"]*)""#).unwrap();
    let mut players = Vec::new();
    for capture in player_re.captures_iter(&xml) {
        let attrs: HashMap<String, String> = attr_re
            .captures_iter(&capture[1])
            .map(|item| (item[1].to_string(), item[2].to_string()))
            .collect();
        let eos = attrs.get("userid").cloned().unwrap_or_default();
        let ttp = save_dir.join("Player").join(format!("EOS_{eos}.ttp"));
        let meta = save_dir.join("Player").join(format!("EOS_{eos}.ttp.meta"));
        let level = parse_level(&meta);
        let progression = parse_ttp_progression(&ttp);
        players.push(Player {
            name: attrs
                .get("playername")
                .cloned()
                .unwrap_or_else(|| "?".to_string()),
            steam: attrs.get("nativeuserid").cloned().unwrap_or_default(),
            login: attrs.get("lastlogin").cloned().unwrap_or_default(),
            pos: attrs.get("position").cloned().unwrap_or_default(),
            coop: capture[1].contains("<acl"),
            level,
            progression,
        });
    }
    players
}

fn parse_level(meta: &Path) -> i32 {
    let text = fs::read_to_string(meta).unwrap_or_default();
    Regex::new(r#"level="(\d+)""#)
        .unwrap()
        .captures(&text)
        .and_then(|capture| capture[1].parse().ok())
        .unwrap_or(0)
}

fn parse_ttp_progression(path: &Path) -> BTreeMap<String, i32> {
    let bytes = fs::read(path).unwrap_or_default();
    let mut hits: Vec<(usize, BTreeMap<String, i32>)> = Vec::new();
    for start in 4..bytes.len().saturating_sub(16) {
        if bytes[start] != 3 {
            continue;
        }
        if let Some((end, values)) = parse_progression_block(&bytes, start) {
            let declared = i32::from_le_bytes(bytes[start - 4..start].try_into().unwrap_or([0; 4]));
            // N-of-M sentinel match: tolerate one game-side rename instead of zeroing
            // the whole save if a single hardcoded key disappears.
            let sentinels = [
                "attstrength",
                "attperception",
                "attfortitude",
                "perkdeadeye",
                "craftingmedical",
                "perkpummelpete",
            ];
            let matched = sentinels
                .iter()
                .filter(|key| values.contains_key(**key))
                .count();
            if declared == (end - start) as i32 && matched >= 3 {
                hits.push((matched, values));
            }
        }
    }
    // 0 hits -> empty (format change). On the rare >1, prefer the MOST credible block
    // (highest sentinel-match count) and, on a tie, the latest in file order — so a
    // forged trailing block can't override the real progression unless it matches more
    // hardcoded keys, while a genuine extra historical block still keeps the newest.
    let max_matched = hits.iter().map(|(m, _)| *m).max().unwrap_or(0);
    hits.into_iter()
        .rfind(|(m, _)| *m == max_matched)
        .map(|(_, v)| v)
        .unwrap_or_default()
}

fn parse_progression_block(bytes: &[u8], start: usize) -> Option<(usize, BTreeMap<String, i32>)> {
    let mut pos = start;
    let version = take_u8(bytes, &mut pos)?;
    if version != 3 {
        return None;
    }
    let _player_level = take_u16(bytes, &mut pos)?;
    let _exp_to_next = take_i32(bytes, &mut pos)?;
    let _unspent = take_u16(bytes, &mut pos)?;
    let count = take_i32(bytes, &mut pos)?;
    if !(50..=1000).contains(&count) {
        return None;
    }
    let mut values = BTreeMap::new();
    for _ in 0..count {
        if take_u8(bytes, &mut pos)? != 1 {
            return None;
        }
        let name = take_dotnet_string(bytes, &mut pos)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return None;
        }
        let level = take_u8(bytes, &mut pos)? as i32;
        let _cost_remaining = take_i32(bytes, &mut pos)?;
        if values.insert(name, level).is_some() {
            return None;
        }
    }
    let _exp_deficit = take_i32(bytes, &mut pos)?;
    Some((pos, values))
}

fn take_u8(bytes: &[u8], pos: &mut usize) -> Option<u8> {
    let value = *bytes.get(*pos)?;
    *pos += 1;
    Some(value)
}

fn take_u16(bytes: &[u8], pos: &mut usize) -> Option<u16> {
    let value = u16::from_le_bytes(bytes.get(*pos..*pos + 2)?.try_into().ok()?);
    *pos += 2;
    Some(value)
}

fn take_i32(bytes: &[u8], pos: &mut usize) -> Option<i32> {
    let value = i32::from_le_bytes(bytes.get(*pos..*pos + 4)?.try_into().ok()?);
    *pos += 4;
    Some(value)
}

fn take_dotnet_string(bytes: &[u8], pos: &mut usize) -> Option<String> {
    let mut length = 0usize;
    let mut shift = 0u32;
    loop {
        if shift >= 35 {
            return None;
        }
        let byte = take_u8(bytes, pos)?;
        length |= ((byte & 0x7f) as usize) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    let raw = bytes.get(*pos..*pos + length)?;
    *pos += length;
    String::from_utf8(raw.to_vec()).ok()
}

#[derive(Clone, Default)]
struct PrefabMeta {
    tier: i32,
    width: i32,
    height: i32,
    depth: i32,
    thumbnail: bool,
    zombies: i32,
    quests: String,
    theme: String,
}

fn placed_prefab_names(xml: &str) -> Vec<String> {
    let re = Regex::new(r#"<decoration[^>]*\bname="([^"]+)""#).unwrap();
    re.captures_iter(xml)
        .map(|capture| capture[1].to_string())
        .collect()
}

fn load_prefab_meta(
    install: Option<&Path>,
    needed: &HashSet<String>,
) -> HashMap<String, PrefabMeta> {
    let mut result = HashMap::new();
    let Some(root) = install.map(|path| path.join("Data/Prefabs/POIs")) else {
        return result;
    };
    let tier_re = Regex::new(r#"DifficultyTier"\s+value="(\d+)""#).unwrap();
    let size_re = Regex::new(r#"PrefabSize"\s+value="(\d+),\s*(\d+),\s*(\d+)""#).unwrap();
    let quest_re = Regex::new(r#"QuestTags"\s+value="([^"]*)""#).unwrap();
    let theme_re = Regex::new(r#"EditorGroups"\s+value="([^"]*)""#).unwrap();
    let group_re = Regex::new(r#"SleeperVolumeGroup"\s+value="([^"]*)""#).unwrap();
    for name in needed {
        let xml_path = root.join(format!("{name}.xml"));
        let Ok(text) = fs::read_to_string(xml_path) else {
            continue;
        };
        let tier = tier_re
            .captures(&text)
            .and_then(|capture| capture[1].parse().ok())
            .unwrap_or(-1);
        let quests = quest_re
            .captures(&text)
            .map(|capture| capture[1].trim().to_string())
            .unwrap_or_default();
        let theme = theme_re
            .captures(&text)
            .map(|capture| capture[1].trim().to_string())
            .unwrap_or_default();
        // Max sleeping zombies the POI defines. SleeperVolumeGroup is (group,min,max)
        // triplets — sum the max of each. NOTE: volume COUNT (SleeperVolumeSize) is a
        // bad danger proxy: e.g. quarry_02 has 78 volumes but most are 0,0 → real max 12.
        let zombies = group_re
            .captures(&text)
            .map(|capture| {
                let toks: Vec<&str> = capture[1].split(',').collect();
                let mut sum = 0i32;
                let mut i = 2;
                while i < toks.len() {
                    sum += toks[i].trim().parse::<i32>().unwrap_or(0);
                    i += 3;
                }
                sum
            })
            .unwrap_or(0);
        let (width, height, depth) = size_re
            .captures(&text)
            .and_then(|capture| {
                Some((
                    capture[1].parse().ok()?,
                    capture[2].parse().ok()?,
                    capture[3].parse().ok()?,
                ))
            })
            .unwrap_or((16, 8, 16));
        result.insert(
            name.clone(),
            PrefabMeta {
                tier,
                width,
                height,
                depth,
                thumbnail: root.join(format!("{name}.jpg")).is_file(),
                zombies,
                quests,
                theme,
            },
        );
    }
    result
}

// Minimal CSV record splitter: handles double-quoted fields with ""-escapes.
// Localization.csv is comma-separated and some english strings are quoted.
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_q {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_q = false;
                }
            } else {
                cur.push(c);
            }
        } else {
            match c {
                '"' => in_q = true,
                ',' => out.push(std::mem::take(&mut cur)),
                _ => cur.push(c),
            }
        }
    }
    out.push(cur);
    out
}

// Real prefab -> in-game / brand name map (e.g. store_book_01 -> "Crack-A-Book",
// trader_bob -> "Trader Bob"). Source: game install Data/Config/Localization.csv,
// rows where File="POI" and Type="POIName"; the name is the `english` column (index 6).
fn load_poi_names(install: Option<&Path>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(root) = install else {
        return map;
    };
    let Ok(text) = fs::read_to_string(root.join("Data/Config/Localization.csv")) else {
        return map;
    };
    for line in text.lines() {
        let fields = parse_csv_line(line);
        if fields.len() > 6 && fields[1] == "POI" && fields[2] == "POIName" {
            let key = fields[0].trim().to_string();
            let name = fields[6].trim().to_string();
            if !key.is_empty() && !name.is_empty() {
                map.insert(key, name);
            }
        }
    }
    map
}

fn parse_prefabs(
    xml: &str,
    meta: &HashMap<String, PrefabMeta>,
    names: &HashMap<String, String>,
) -> Vec<Poi> {
    let re = Regex::new(
        r#"<decoration[^>]*\bname="([^"]+)"[^>]*\bposition="(-?\d+),(-?\d+),(-?\d+)"[^>]*\brotation="(\d+)""#,
    )
    .unwrap();
    let filler =
        Regex::new(r"^(part_|rwg_tile_|rubble_|remnant_|wilderness_filler_|lot_|deco_|crater_)")
            .unwrap();
    let mut pois = Vec::new();
    for capture in re.captures_iter(xml) {
        let name = capture[1].to_string();
        if filler.is_match(&name) {
            continue;
        }
        let info = meta.get(&name).cloned().unwrap_or_default();
        let rotation = capture[5].parse().unwrap_or(0);
        let (width, depth) = if rotation % 2 == 1 {
            (info.depth, info.width)
        } else {
            (info.width, info.depth)
        };
        // prefabs.xml position is the SW corner (min x/z) of the footprint; the box
        // is centred on (x,z), so shift by half the rotation-adjusted size to land it
        // on the real footprint instead of half-a-building to the south-west.
        pois.push(Poi {
            name: name.clone(),
            x: capture[2].parse::<i32>().unwrap_or(0).saturating_add(width / 2),
            y: capture[3].parse().unwrap_or(0),
            z: capture[4].parse::<i32>().unwrap_or(0).saturating_add(depth / 2),
            tier: info.tier,
            rotation,
            width,
            depth,
            height: info.height,
            thumbnail: info
                .thumbnail
                .then(|| format!("/poi/{}.jpg", encode_segment(&name))),
            zombies: info.zombies,
            quests: info.quests,
            theme: info.theme,
            ingame: names.get(&name).cloned().unwrap_or_default(),
        });
    }
    pois
}

struct MapInfo {
    size: i32,
    seed: String,
    ver: String,
    rwg: String,
    gen: BTreeMap<String, String>,
}

fn parse_map_info(path: &Path) -> MapInfo {
    let text = fs::read_to_string(path).unwrap_or_default();
    let value = |key: &str| {
        Regex::new(&format!(r#"name="{key}"\s+value="([^"]*)""#))
            .unwrap()
            .captures(&text)
            .map(|capture| capture[1].to_string())
            .unwrap_or_default()
    };
    let size = value("HeightMapSize")
        .split(',')
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or(10240);
    let mut gen = BTreeMap::new();
    for key in [
        "Forest",
        "BurntForest",
        "Desert",
        "Snow",
        "Wasteland",
        "Rivers",
        "Towns",
    ] {
        gen.insert(key.to_string(), value(key));
    }
    MapInfo {
        size,
        seed: value("Seed"),
        ver: value("GameVersion"),
        rwg: value("RandomGeneratedWorld"),
        gen,
    }
}

fn sample_heightmap(path: &Path, size: usize) -> Result<HeightMap, String> {
    // Guard against a bogus HeightMapSize: < N would read past the row buffer (OOB panic),
    // a huge value would trigger a giant allocation/abort. Caller uses .ok() → drops the
    // heightmap instead of crashing the scan thread.
    if !(HEIGHTMAP_N..=32768).contains(&size) {
        return Err(format!("Unplausible HeightMapSize: {size}"));
    }
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    // dtm.raw is a headerless size×size grid of u16 → exactly size*size*2 bytes. If the length
    // doesn't match (corrupt or guessed HeightMapSize from a malformed map_info.xml), reject so
    // we drop the heightmap instead of sampling at the wrong stride and rendering a garbled map.
    let expected = (size as u64).saturating_mul(size as u64).saturating_mul(2);
    let actual = file.metadata().map(|m| m.len()).unwrap_or(0);
    if actual != expected {
        return Err(format!(
            "dtm.raw Größe {actual} != erwartet {expected} (HeightMapSize {size} unplausibel)"
        ));
    }
    let step = (size / HEIGHTMAP_N).max(1);
    let row_bytes = size * 2;
    let mut data = vec![0u8; HEIGHTMAP_N * HEIGHTMAP_N];
    let mut row = vec![0u8; row_bytes];
    let mut mn = u8::MAX;
    let mut mx = u8::MIN;
    for y in 0..HEIGHTMAP_N {
        file.seek(SeekFrom::Start((y * step * row_bytes) as u64))
            .map_err(|e| e.to_string())?;
        file.read_exact(&mut row).map_err(|e| e.to_string())?;
        for x in 0..HEIGHTMAP_N {
            let offset = x * step * 2;
            let raw = u16::from_le_bytes([row[offset], row[offset + 1]]);
            let height = (raw >> 8) as u8;
            data[y * HEIGHTMAP_N + x] = height;
            mn = mn.min(height);
            mx = mx.max(height);
        }
    }
    Ok(HeightMap {
        n: HEIGHTMAP_N,
        mn,
        mx,
        d: STANDARD.encode(data),
    })
}

/// Decode splat4 and max-pool its BLUE channel (the game's water mask, faint ≤30)
/// down to n×n. Max-pool (not average) keeps 1-px rivers from vanishing. Returns
/// None if the layer holds no water at all.
fn water_mask(path: &Path, n: usize) -> Option<WaterMask> {
    // Never decode the full 10240² splat (~400MB RGBA in RAM). image_dimensions reads
    // only the header, so we bail before allocating if a full-res splat slipped through
    // (the caller already prefers the 5120² _half variant).
    if let Ok((w, h)) = image::image_dimensions(path) {
        if w > 6144 || h > 6144 {
            return None;
        }
    }
    let img = image::open(path).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    let (sx, sy) = ((w as usize / n).max(1), (h as usize / n).max(1));
    let mut out = vec![0u8; n * n];
    for oy in 0..n {
        for ox in 0..n {
            let mut mx = 0u8;
            for dy in 0..sy {
                for dx in 0..sx {
                    let px = (ox * sx + dx) as u32;
                    let py = (oy * sy + dy) as u32;
                    if px < w && py < h {
                        let b = img.get_pixel(px, py)[2];
                        if b > mx {
                            mx = b;
                        }
                    }
                }
            }
            out[oy * n + ox] = mx;
        }
    }
    if out.iter().all(|&v| v < 2) {
        return None;
    }
    Some(WaterMask {
        n,
        d: STANDARD.encode(&out),
    })
}

fn file_url_if_exists(dir: &Path, key: &str, name: &str) -> Option<String> {
    dir.join(name)
        .is_file()
        .then(|| format!("/world/{key}/{name}"))
}

fn encode_segment(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}

fn decode_segment(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

fn serve_world_asset(request: tiny_http::Request, paths: &Paths, url: &str) -> Result<(), String> {
    let parts: Vec<_> = url.trim_start_matches('/').split('/').collect();
    if parts.len() != 3 {
        return respond_status_json(
            request,
            StatusCode(404),
            &json!({"error":"Ungültiger Pfad"}),
        );
    }
    let world = decode_segment(parts[1]);
    // validate AFTER percent-decoding: `..%5c..%5c` only becomes `..\..\` here
    if safe_segment(&world).is_err() {
        return respond_status_json(request, StatusCode(403), &json!({"error":"Ungültiger Welt-Name"}));
    }
    let file = parts[2];
    if !matches!(
        file,
        "biomes.png"
            | "splat3.png"
            | "splat3_half.png"
            | "splat3_processed.png"
            | "splat4.png"
            | "splat4_half.png"
    ) {
        return respond_status_json(request, StatusCode(403), &json!({"error":"Datei gesperrt"}));
    }
    let root = paths.appdata.join("GeneratedWorlds");
    let path = root.join(&world).join(file);
    // defence in depth: a resolved path must stay under the worlds root. Fail CLOSED
    // — if canonicalize errors (missing file/permission), reject instead of serving.
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(rp), Ok(rr)) if rp.starts_with(&rr) => {}
        _ => {
            return respond_status_json(request, StatusCode(403), &json!({"error":"Pfad gesperrt"}))
        }
    }
    serve_file(request, &path, "image/png")
}

fn serve_poi_asset(request: tiny_http::Request, paths: &Paths, url: &str) -> Result<(), String> {
    let Some(install) = &paths.install else {
        return respond_status_json(request, StatusCode(404), &json!({"error":"Spiel fehlt"}));
    };
    let name = url
        .trim_start_matches("/poi/")
        .strip_suffix(".jpg")
        .unwrap_or("");
    let decoded = decode_segment(name);
    if decoded.is_empty() || safe_segment(&decoded).is_err() {
        return respond_status_json(request, StatusCode(403), &json!({"error":"Ungültiger Name"}));
    }
    let root = install.join("Data/Prefabs/POIs");
    let path = root.join(format!("{decoded}.jpg"));
    // same fail-closed containment guard as serve_world_asset
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(rp), Ok(rr)) if rp.starts_with(&rr) => {}
        _ => {
            return respond_status_json(request, StatusCode(403), &json!({"error":"Pfad gesperrt"}))
        }
    }
    serve_file(request, &path, "image/jpeg")
}

fn serve_file(request: tiny_http::Request, path: &Path, content_type: &str) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serve_bytes(request, data, content_type)
}

fn serve_bytes(
    request: tiny_http::Request,
    data: Vec<u8>,
    content_type: &str,
) -> Result<(), String> {
    let mut response = Response::from_data(data);
    response.add_header(Header::from_bytes("Content-Type", content_type).unwrap());
    response.add_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
    // Anti-clickjacking: a <meta> CSP cannot set frame-ancestors, so deny framing via header.
    // Stops a malicious local page from iframing the loopback UI to UI-redress the write buttons.
    response.add_header(Header::from_bytes("X-Frame-Options", "DENY").unwrap());
    request.respond(response).map_err(|e| e.to_string())
}

fn respond_json<T: Serialize>(request: tiny_http::Request, data: &T) -> Result<(), String> {
    respond_status_json(request, StatusCode(200), data)
}

fn respond_status_json<T: Serialize>(
    request: tiny_http::Request,
    status: StatusCode,
    data: &T,
) -> Result<(), String> {
    let body = serde_json::to_vec(data).map_err(|e| e.to_string())?;
    let mut response = Response::from_data(body).with_status_code(status);
    response
        .add_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap());
    response.add_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
    request.respond(response).map_err(|e| e.to_string())
}

// ===================== gameOptions.sdf WRITER =====================
// Byte-faithful re-encode. Untouched string values keep their original raw
// bytes so an unchanged save round-trips identically; only edited fields are
// re-encoded. A round-trip guard refuses to write if re-serialisation of the
// untouched parse differs from the original file.

#[derive(Clone)]
enum SdfVal {
    Int(i32),
    Str { decoded: String, raw: Vec<u8> },
    Bool(bool),
    Float(f32),
}

#[derive(Clone)]
struct SdfEntry {
    kind: u8,
    key: String,
    val: SdfVal,
}

fn parse_sdf_entries(bytes: &[u8]) -> Result<Vec<SdfEntry>, String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let kind = bytes[pos];
        pos += 1;
        if pos + 3 > bytes.len() {
            return Err("Längenfeld abgeschnitten".into());
        }
        let key_len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
        pos += 3;
        if pos + key_len > bytes.len() {
            return Err("Schlüssel abgeschnitten".into());
        }
        let key = String::from_utf8_lossy(&bytes[pos..pos + key_len]).into_owned();
        pos += key_len;
        let val = match kind {
            1 if pos + 4 <= bytes.len() => {
                let value = i32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
                pos += 4;
                SdfVal::Int(value)
            }
            2 if pos + 3 <= bytes.len() => {
                let len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
                pos += 3;
                if pos + len > bytes.len() {
                    return Err("String-Wert abgeschnitten".into());
                }
                let raw = bytes[pos..pos + len].to_vec();
                pos += len;
                let decoded = STANDARD
                    .decode(&raw)
                    .ok()
                    .and_then(|v| String::from_utf8(v).ok())
                    .unwrap_or_else(|| String::from_utf8_lossy(&raw).into_owned());
                SdfVal::Str { decoded, raw }
            }
            3 if pos < bytes.len() => {
                let value = bytes[pos] != 0;
                pos += 1;
                SdfVal::Bool(value)
            }
            4 if pos + 4 <= bytes.len() => {
                let value = f32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
                pos += 4;
                SdfVal::Float(value)
            }
            _ => return Err(format!("Unbekannter Typ {kind} bei Offset {pos}")),
        };
        out.push(SdfEntry { kind, key, val });
    }
    Ok(out)
}

/// Length prefix layout mirrors the game's: u16 little-endian (low, high) followed
/// by a redundant copy of the low byte — the parser reads the u16 and skips byte 3.
/// Only the low 16 bits are used, so a single field >= 65536 bytes would truncate;
/// in practice keys and the base64 SandboxCode are far below that. The write_settings
/// round-trip guard (serialize == original) fails closed if this ever mis-encodes.
fn write_len(out: &mut Vec<u8>, len: usize) {
    out.push((len & 0xff) as u8);
    out.push(((len >> 8) & 0xff) as u8);
    out.push((len & 0xff) as u8);
}

fn serialize_sdf(entries: &[SdfEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in entries {
        out.push(entry.kind);
        write_len(&mut out, entry.key.len());
        out.extend_from_slice(entry.key.as_bytes());
        match &entry.val {
            SdfVal::Int(value) => out.extend_from_slice(&value.to_le_bytes()),
            SdfVal::Str { raw, .. } => {
                write_len(&mut out, raw.len());
                out.extend_from_slice(raw);
            }
            SdfVal::Bool(value) => out.push(if *value { 1 } else { 0 }),
            SdfVal::Float(value) => out.extend_from_slice(&value.to_le_bytes()),
        }
    }
    out
}

/// Set (or append) one sandbox option in the SandboxCode triplet string.
/// Header char is preserved; only the value char of the matching triplet is
/// replaced, or a new triplet appended for a not-yet-overridden option.
fn patch_sandbox(code: &str, name: &str, idx: i32) -> Result<String, String> {
    let opt = SANDORDER
        .iter()
        .position(|&n| n == name)
        .ok_or_else(|| format!("Unbekannte Sandbox-Option: {name}"))?;
    if !(0..=25).contains(&idx) {
        return Err(format!("Wert-Index {idx} außerhalb 0-25 für {name}"));
    }
    let hi = (opt / 26) as u8;
    let mid = (opt % 26) as u8;
    let bytes = code.as_bytes();
    if bytes.is_empty() {
        return Err("Leerer SandboxCode".into());
    }
    let header = bytes[0];
    let body = &bytes[1..];
    if !body.len().is_multiple_of(3) {
        return Err("SandboxCode-Länge nicht durch 3 teilbar".into());
    }
    let mut out: Vec<u8> = vec![header];
    let mut found = false;
    let mut j = 0;
    while j + 3 <= body.len() {
        let (c0, c1, c2) = (body[j], body[j + 1], body[j + 2]);
        let o = (c0.wrapping_sub(b'A') as usize) * 26 + (c1.wrapping_sub(b'A') as usize);
        out.push(c0);
        out.push(c1);
        if o == opt {
            out.push(b'A' + idx as u8);
            found = true;
        } else {
            out.push(c2);
        }
        j += 3;
    }
    if !found {
        out.push(b'A' + hi);
        out.push(b'A' + mid);
        out.push(b'A' + idx as u8);
    }
    String::from_utf8(out).map_err(|e| e.to_string())
}

fn apply_plain(entry: &mut SdfEntry, value: &Value) -> Result<(), String> {
    entry.val = match &entry.val {
        SdfVal::Int(_) => SdfVal::Int(
            value
                .as_i64()
                .ok_or_else(|| format!("{} erwartet Zahl", entry.key))? as i32,
        ),
        SdfVal::Bool(_) => SdfVal::Bool(
            value
                .as_bool()
                .or_else(|| value.as_i64().map(|n| n != 0))
                .ok_or_else(|| format!("{} erwartet bool", entry.key))?,
        ),
        SdfVal::Float(_) => SdfVal::Float(
            value
                .as_f64()
                .ok_or_else(|| format!("{} erwartet Zahl", entry.key))? as f32,
        ),
        SdfVal::Str { .. } => {
            let s = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            SdfVal::Str {
                raw: STANDARD.encode(s.as_bytes()).into_bytes(),
                decoded: s,
            }
        }
    };
    Ok(())
}

fn safe_segment(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.contains("..")
        || value.contains('/')
        || value.contains('\\')
        || value.contains(':')
    {
        return Err("Ungültiger Welt-/Save-Name".into());
    }
    Ok(())
}

/// Plain-number (min,max) bounds from the baked refdata, for write-time validation.
/// The UI clamps too, but the HTTP endpoint can be hit directly, so the writer
/// enforces the same ranges as defence in depth.
fn refdata_plain_bounds() -> HashMap<String, (Option<i64>, Option<i64>)> {
    let mut map = HashMap::new();
    if let Ok(v) = serde_json::from_str::<Value>(REFDATA) {
        if let Some(arr) = v.get("plain").and_then(|p| p.as_array()) {
            for o in arr {
                if let Some(name) = o.get("name").and_then(|n| n.as_str()) {
                    let mn = o.get("min").and_then(|x| x.as_i64());
                    let mx = o.get("max").and_then(|x| x.as_i64());
                    if mn.is_some() || mx.is_some() {
                        map.insert(name.to_string(), (mn, mx));
                    }
                }
            }
        }
    }
    map
}

/// Valid value-count per sandbox option from the baked refdata. A SandboxCode value
/// index must be < this count; the UI selects already enforce it, the writer mirrors it.
fn refdata_sandbox_counts() -> HashMap<String, usize> {
    let mut map = HashMap::new();
    if let Ok(v) = serde_json::from_str::<Value>(REFDATA) {
        if let Some(arr) = v.get("sandbox").and_then(|p| p.as_array()) {
            for o in arr {
                if let (Some(name), Some(opts)) = (
                    o.get("name").and_then(|n| n.as_str()),
                    o.get("options").and_then(|x| x.as_array()),
                ) {
                    map.insert(name.to_string(), opts.len());
                }
            }
        }
    }
    map
}

/// Keep only the newest `keep` gameOptions.sdf.bak.* files in a save dir; delete the
/// rest. Backups are made on every write AND every restore and were never pruned, so
/// they grew without bound in the user's real save folder.
fn prune_backups(dir: &Path, keep: usize) {
    let mut baks: Vec<(u128, PathBuf)> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let name = p.file_name()?.to_string_lossy().into_owned();
                let stamp = name.strip_prefix("gameOptions.sdf.bak.")?;
                // Skip non-numeric suffixes (filter_map drops None) so a user-renamed backup is
                // never sorted to stamp 0 and pruned first — only real millis-stamped ones prune.
                Some((stamp.parse::<u128>().ok()?, p))
            })
            .collect(),
        Err(_) => return,
    };
    if baks.len() <= keep {
        return;
    }
    baks.sort_by_key(|(s, _)| *s);
    let remove = baks.len() - keep;
    for (_, p) in baks.into_iter().take(remove) {
        let _ = fs::remove_file(p);
    }
}

fn write_settings(paths: &Paths, body: &str) -> Result<Value, String> {
    let req: Value = serde_json::from_str(body).map_err(|e| format!("JSON-Fehler: {e}"))?;
    let world = req["world"].as_str().ok_or("'world' fehlt")?;
    let save = req["save"].as_str().ok_or("'save' fehlt")?;
    safe_segment(world)?;
    safe_segment(save)?;

    let dir = paths.appdata.join("Saves").join(world).join(save);
    let sdf = dir.join("gameOptions.sdf");
    if !sdf.is_file() {
        return Err(format!("gameOptions.sdf nicht gefunden: {}", sdf.display()));
    }

    let original = fs::read(&sdf).map_err(|e| format!("Lesen fehlgeschlagen: {e}"))?;
    let mut entries = parse_sdf_entries(&original)?;

    // Safety: an untouched parse must serialise back byte-identically, otherwise
    // our encoder does not understand this file and we must NOT write it.
    if serialize_sdf(&entries) != original {
        return Err(
            "Sicherheits-Check fehlgeschlagen: Datei-Format weicht ab — es wurde NICHTS geschrieben."
                .into(),
        );
    }

    let mut changed = 0usize;
    let mut details: Vec<String> = Vec::new();

    let plain_bounds = refdata_plain_bounds();
    if let Some(plain) = req.get("plain").and_then(|v| v.as_object()) {
        for (key, value) in plain {
            match entries.iter_mut().find(|e| &e.key == key) {
                Some(entry) => {
                    if let (Some((mn, mx)), Some(n)) = (plain_bounds.get(key), value.as_i64()) {
                        if mn.is_some_and(|lo| n < lo) || mx.is_some_and(|hi| n > hi) {
                            return Err(format!("{key}: Wert {n} außerhalb des erlaubten Bereichs"));
                        }
                    }
                    apply_plain(entry, value)?;
                    changed += 1;
                    details.push(key.clone());
                }
                None => return Err(format!("Schlüssel nicht in dieser Welt: {key}")),
            }
        }
    }

    let sandbox_counts = refdata_sandbox_counts();
    if let Some(sandbox) = req.get("sandbox").and_then(|v| v.as_object()) {
        if !sandbox.is_empty() {
            let entry = entries
                .iter_mut()
                .find(|e| e.key == "SandboxCode")
                .ok_or("Diese Welt hat keinen SandboxCode (keine 3.0-Welt) — Sandbox-Optionen nicht schreibbar")?;
            if let SdfVal::Str { decoded, raw } = &mut entry.val {
                let mut code = decoded.clone();
                for (name, idx_val) in sandbox {
                    let idx = idx_val.as_i64().ok_or_else(|| format!("{name}: Index keine Zahl"))? as i32;
                    if let Some(cnt) = sandbox_counts.get(name) {
                        if idx < 0 || (idx as usize) >= *cnt {
                            return Err(format!(
                                "{name}: Index {idx} außerhalb 0..{}",
                                cnt.saturating_sub(1)
                            ));
                        }
                    }
                    code = patch_sandbox(&code, name, idx)?;
                    changed += 1;
                    details.push(format!("{name}=#{idx}"));
                }
                *raw = STANDARD.encode(code.as_bytes()).into_bytes();
                *decoded = code;
            } else {
                return Err("SandboxCode hat unerwarteten Typ".into());
            }
        }
    }

    if changed == 0 {
        return Err("Keine Änderungen übermittelt".into());
    }

    let new_bytes = serialize_sdf(&entries);

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let backup = dir.join(format!("gameOptions.sdf.bak.{stamp}"));
    fs::copy(&sdf, &backup).map_err(|e| format!("Backup fehlgeschlagen (nichts geschrieben): {e}"))?;
    prune_backups(&dir, 10);
    // atomic replace: write a temp file in the same dir, then rename over the live file,
    // so a crash/standby mid-write can never leave a half-written gameOptions.sdf.
    let tmp = dir.join("gameOptions.sdf.tmp");
    fs::write(&tmp, &new_bytes).map_err(|e| format!("Schreiben fehlgeschlagen: {e}"))?;
    fs::rename(&tmp, &sdf).map_err(|e| format!("Ersetzen fehlgeschlagen: {e}"))?;

    Ok(json!({
        "ok": true,
        "changed": changed,
        "details": details,
        "backup": backup.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "backupPath": backup.display().to_string(),
        "bytes": new_bytes.len(),
    }))
}

/// Restore the most recent gameOptions.sdf.bak.* over the live file — the UI's undo.
fn restore_settings(paths: &Paths, body: &str) -> Result<Value, String> {
    let req: Value = serde_json::from_str(body).map_err(|e| format!("JSON-Fehler: {e}"))?;
    let world = req["world"].as_str().ok_or("'world' fehlt")?;
    let save = req["save"].as_str().ok_or("'save' fehlt")?;
    safe_segment(world)?;
    safe_segment(save)?;
    let dir = paths.appdata.join("Saves").join(world).join(save);
    let sdf = dir.join("gameOptions.sdf");
    // newest backup by the unix stamp suffix
    let mut backups: Vec<(u64, std::path::PathBuf)> = fs::read_dir(&dir)
        .map_err(|e| format!("Save-Ordner nicht lesbar: {e}"))?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy().into_owned();
            let stamp = name.strip_prefix("gameOptions.sdf.bak.")?;
            Some((stamp.parse::<u64>().ok()?, path))
        })
        .collect();
    backups.sort_by_key(|(stamp, _)| *stamp);
    let Some((_, newest)) = backups.pop() else {
        return Err("Kein Backup gefunden — es wurde noch nichts geschrieben.".into());
    };
    // Validate the backup actually parses before trusting it over the live file.
    let backup_bytes = fs::read(&newest).map_err(|e| format!("Backup nicht lesbar: {e}"))?;
    parse_sdf_entries(&backup_bytes)
        .map_err(|e| format!("Backup beschädigt, Restore abgebrochen: {e}"))?;
    // Snapshot the current live file first, so the restore is itself undoable
    // (a subsequent restore picks up this newest snapshot).
    if sdf.is_file() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = fs::copy(&sdf, dir.join(format!("gameOptions.sdf.bak.{stamp}")));
        prune_backups(&dir, 10);
    }
    fs::copy(&newest, &sdf).map_err(|e| format!("Restore fehlgeschlagen: {e}"))?;
    Ok(json!({
        "ok": true,
        "restored": newest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dotnet_string(text: &str) -> Vec<u8> {
        assert!(text.len() < 128);
        let mut out = vec![text.len() as u8];
        out.extend_from_slice(text.as_bytes());
        out
    }

    #[test]
    fn parses_progression_v3_block() {
        let mut block = vec![3];
        block.extend_from_slice(&24u16.to_le_bytes());
        block.extend_from_slice(&1234i32.to_le_bytes());
        block.extend_from_slice(&2u16.to_le_bytes());
        let mut entries = vec![
            ("attstrength", 5u8),
            ("attperception", 1u8),
            ("perkdeadeye", 0u8),
            ("craftingmedical", 12u8),
        ];
        let dummy_names: Vec<String> = (0..46).map(|index| format!("perkdummy{index}")).collect();
        for name in &dummy_names {
            entries.push((name.as_str(), 0));
        }
        block.extend_from_slice(&(entries.len() as i32).to_le_bytes());
        for (name, level) in entries {
            block.push(1);
            block.extend(dotnet_string(name));
            block.push(level);
            block.extend_from_slice(&0i32.to_le_bytes());
        }
        block.extend_from_slice(&0i32.to_le_bytes());

        let (end, values) = parse_progression_block(&block, 0).expect("valid block");
        assert_eq!(end, block.len());
        assert_eq!(values["attstrength"], 5);
        assert_eq!(values["craftingmedical"], 12);
    }

    #[test]
    fn rejects_wrong_progression_version() {
        assert!(parse_progression_block(&[2, 0, 0, 0], 0).is_none());
    }

    #[test]
    fn sdf_roundtrip_synthetic() {
        // [type][u16 len LE][redundant low byte][key][value]
        let mut bytes = Vec::new();
        // int key
        bytes.push(1u8);
        bytes.extend_from_slice(&[10, 0, 10]);
        bytes.extend_from_slice(b"ServerPort");
        bytes.extend_from_slice(&26900i32.to_le_bytes());
        // string key (base64 of "Europe")
        let b64 = STANDARD.encode(b"Europe");
        bytes.push(2u8);
        bytes.extend_from_slice(&[6, 0, 6]);
        bytes.extend_from_slice(b"Region");
        bytes.push(b64.len() as u8);
        bytes.push(0);
        bytes.push(b64.len() as u8);
        bytes.extend_from_slice(b64.as_bytes());
        // bool
        bytes.push(3u8);
        bytes.extend_from_slice(&[9, 0, 9]);
        bytes.extend_from_slice(b"BuildMode");
        bytes.push(0);

        let entries = parse_sdf_entries(&bytes).expect("parse");
        assert_eq!(serialize_sdf(&entries), bytes, "round-trip must be byte-identical");
    }

    #[test]
    fn patch_sandbox_replace_and_append() {
        // ZombieMove is option index 34 -> hi=1(B) mid=8(I); value Nightmare=4(E) => "BIE"
        let code = "ABIE"; // header 'A' + one triplet BIE
        // change ZombieMove to Sprint (index 3 = 'D')
        let changed = patch_sandbox(code, "ZombieMove", 3).unwrap();
        assert_eq!(changed, "ABID");
        // append a brand-new override: XPMultiplier is index 18 -> hi=0(A) mid=18(S); idx5 => 'F'
        let appended = patch_sandbox(&changed, "XPMultiplier", 5).unwrap();
        assert_eq!(appended, "ABIDASF");
        // header preserved, length multiple of 3 + 1
        assert_eq!(appended.as_bytes()[0], b'A');
        assert_eq!((appended.len() - 1) % 3, 0);
    }

    #[test]
    fn patch_sandbox_rejects_bad_index() {
        assert!(patch_sandbox("ABIE", "ZombieMove", 26).is_err());
        assert!(patch_sandbox("ABIE", "NoSuchOption", 1).is_err());
    }

    #[test]
    fn real_save_roundtrips_if_present() {
        // Guard the live write path against the user's actual save format.
        let appdata = match env::var_os("APPDATA") {
            Some(value) => PathBuf::from(value).join("7DaysToDie"),
            None => return,
        };
        let candidates = [
            appdata.join("Saves/Putipovu Valley/3.0 beta/gameOptions.sdf"),
            appdata.join("Saves/Epila Territory/Modet/gameOptions.sdf"),
        ];
        let mut tested = 0;
        for path in candidates {
            if let Ok(bytes) = fs::read(&path) {
                let entries = parse_sdf_entries(&bytes)
                    .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
                assert_eq!(
                    serialize_sdf(&entries),
                    bytes,
                    "real save must round-trip byte-identical: {}",
                    path.display()
                );
                tested += 1;
            }
        }
        eprintln!("real_save_roundtrips_if_present: {tested} save(s) verified");
    }

    // Guardrail against silent save corruption: the SandboxCode wire format encodes
    // each option by its INDEX, so SANDORDER (the writer) and refdata.json sandbox[]
    // (the UI) must agree on order and length. Any drift here would write the wrong
    // setting into a real gameOptions.sdf with no error.
    #[test]
    fn sandorder_matches_refdata() {
        let v: Value = serde_json::from_str(REFDATA).expect("refdata.json parses");
        let arr = v["sandbox"].as_array().expect("refdata sandbox[] is an array");
        assert_eq!(arr.len(), SANDORDER.len(), "sandbox count drift");
        for entry in arr {
            let idx = entry["idx"].as_u64().expect("sandbox entry has idx") as usize;
            let name = entry["name"].as_str().expect("sandbox entry has name");
            assert_eq!(
                SANDORDER.get(idx),
                Some(&name),
                "SANDORDER drift at idx {idx}: refdata says {name}"
            );
        }
    }
}
