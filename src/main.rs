#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{engine::general_purpose::STANDARD, Engine};
use percent_encoding::percent_decode_str;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tiny_http::{Header, Method, Response, Server, StatusCode};

const ADDRESS: &str = "127.0.0.1:17873";
const HEIGHTMAP_N: usize = 256;
const APP_HTML: &str = include_str!("../7DtD_Skill_Tracker.html");

#[derive(Clone, Serialize)]
struct Player {
    name: String,
    steam: String,
    login: String,
    pos: String,
    coop: bool,
    level: i32,
    progression: BTreeMap<String, i32>,
}

#[derive(Clone, Serialize)]
struct Save {
    id: String,
    world: String,
    save: String,
    settings: Map<String, Value>,
    pl: Vec<Player>,
    scanned: bool,
    #[serde(rename = "hasMap")]
    has_map: bool,
}

#[derive(Clone, Serialize)]
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
}

#[derive(Clone, Serialize)]
struct HeightMap {
    n: usize,
    mn: u8,
    mx: u8,
    d: String,
}

#[derive(Clone, Serialize)]
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
}

#[derive(Clone, Serialize)]
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
            eprintln!("[7DtD] Serverstart fehlgeschlagen: {error}");
            let _ = open::that(format!("http://{ADDRESS}"));
            return;
        }
    };
    let cache: Arc<Mutex<Option<ScanData>>> = Arc::new(Mutex::new(None));
    println!("[7DtD] Rust Companion läuft: http://{ADDRESS}");

    thread::spawn(|| {
        thread::sleep(Duration::from_millis(350));
        let _ = open::that(format!("http://{ADDRESS}"));
    });

    for request in server.incoming_requests() {
        let paths = paths.clone();
        let cache = cache.clone();
        if let Err(error) = handle(request, &paths, &cache) {
            eprintln!("[7DtD] Request-Fehler: {error}");
        }
    }
}

fn handle(
    request: tiny_http::Request,
    paths: &Paths,
    cache: &Arc<Mutex<Option<ScanData>>>,
) -> Result<(), String> {
    let raw_url = request.url().to_string();
    let path = raw_url.split('?').next().unwrap_or("/");
    match (request.method(), path) {
        (&Method::Get, "/") | (&Method::Get, "/index.html") => serve_bytes(
            request,
            APP_HTML.as_bytes().to_vec(),
            "text/html; charset=utf-8",
        ),
        (&Method::Get, "/api/health") => respond_json(request, &json!({"ok": true})),
        (&Method::Get, "/api/scan") | (&Method::Post, "/api/scan") => match scan(paths) {
            Ok(data) => {
                *cache.lock().map_err(|e| e.to_string())? = Some(data.clone());
                respond_json(request, &data)
            }
            Err(error) => respond_status_json(
                request,
                StatusCode(500),
                &json!({"ok": false, "error": error}),
            ),
        },
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
        _ if path.starts_with("/world/") => serve_world_asset(request, paths, path),
        _ if path.starts_with("/poi/") => serve_poi_asset(request, paths, path),
        _ => respond_status_json(
            request,
            StatusCode(404),
            &json!({"ok": false, "error": "Nicht gefunden"}),
        ),
    }
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

    let mut saves = Vec::new();
    for world in read_dirs(&saves_root)? {
        let world_dir = saves_root.join(&world);
        for save_name in read_dirs(&world_dir)? {
            let save_dir = world_dir.join(&save_name);
            let sdf = save_dir.join("gameOptions.sdf");
            if !sdf.is_file() {
                continue;
            }
            let settings = parse_sdf(&fs::read(&sdf).unwrap_or_default());
            let players = parse_players(&save_dir);
            saves.push(Save {
                id: format!("{world} / {save_name}"),
                world: world.clone(),
                save: save_name,
                settings,
                pl: players,
                scanned: true,
                has_map: worlds_root.join(&world).is_dir(),
            });
        }
    }
    saves.sort_by(|a, b| a.id.to_lowercase().cmp(&b.id.to_lowercase()));

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
        let pois = parse_prefabs(&prefab_text, &prefab_meta);
        let dtm = dir.join("dtm.raw");
        let hm = if dtm.is_file() {
            sample_heightmap(&dtm, map_info.size as usize).ok()
        } else {
            None
        };
        maps.insert(
            world.clone(),
            WorldMap {
                world: world.clone(),
                key: key.clone(),
                img: format!("/world/{key}/biomes.png"),
                roads: file_url_if_exists(&dir, &key, "splat3.png"),
                water: file_url_if_exists(&dir, &key, "splat4.png"),
                size: map_info.size,
                seed: map_info.seed,
                ver: map_info.ver,
                rwg: map_info.rwg,
                gen: map_info.gen,
                pois,
                hm,
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
    let mut hits = Vec::new();
    for start in 4..bytes.len().saturating_sub(16) {
        if bytes[start] != 3 {
            continue;
        }
        if let Some((end, values)) = parse_progression_block(&bytes, start) {
            let declared = i32::from_le_bytes(bytes[start - 4..start].try_into().unwrap_or([0; 4]));
            let valid_names = values.contains_key("attstrength")
                && values.contains_key("attperception")
                && values.contains_key("perkdeadeye")
                && values.contains_key("craftingmedical");
            if declared == (end - start) as i32 && valid_names {
                hits.push(values);
            }
        }
    }
    if hits.len() == 1 {
        hits.remove(0)
    } else {
        BTreeMap::new()
    }
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
    for name in needed {
        let xml_path = root.join(format!("{name}.xml"));
        let Ok(text) = fs::read_to_string(xml_path) else {
            continue;
        };
        let tier = tier_re
            .captures(&text)
            .and_then(|capture| capture[1].parse().ok())
            .unwrap_or(-1);
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
            },
        );
    }
    result
}

fn parse_prefabs(xml: &str, meta: &HashMap<String, PrefabMeta>) -> Vec<Poi> {
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
        pois.push(Poi {
            name: name.clone(),
            x: capture[2].parse().unwrap_or(0),
            y: capture[3].parse().unwrap_or(0),
            z: capture[4].parse().unwrap_or(0),
            tier: info.tier,
            rotation,
            width,
            depth,
            height: info.height,
            thumbnail: info
                .thumbnail
                .then(|| format!("/poi/{}.jpg", encode_segment(&name))),
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
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
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
    let file = parts[2];
    if !matches!(
        file,
        "biomes.png" | "splat3.png" | "splat3_processed.png" | "splat4.png"
    ) {
        return respond_status_json(request, StatusCode(403), &json!({"error":"Datei gesperrt"}));
    }
    let path = paths.appdata.join("GeneratedWorlds").join(world).join(file);
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
    let path = install
        .join("Data/Prefabs/POIs")
        .join(format!("{}.jpg", decode_segment(name)));
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
}
