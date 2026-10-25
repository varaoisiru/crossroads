use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::{Arc, Mutex}};
use tower_http::services::ServeDir;

const UA: &str = "Crossroads/0.1 (local food finder, Justin personal use)";

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct Place {
    place_key: String,
    name: String,
    lat: f64,
    lon: f64,
    distance_m: i64,
    category: String,
    cuisine: Option<String>,
    opening_hours: Option<String>,
    website: Option<String>,
    phone: Option<String>,
    address: Option<String>,
    branch: Option<String>,
    diet: Vec<String>,
    menu_url: Option<String>,
    menu_search_url: String,
    deals_search_url: String,
}

#[derive(Serialize)]
struct SearchResponse {
    query: String,
    resolved_name: Option<String>,
    lat: f64,
    lon: f64,
    radius_m: u32,
    count: usize,
    places: Vec<Place>,
}

#[derive(Deserialize)]
struct SearchParams { q: Option<String>, radius: Option<u32> }

#[derive(Serialize)]
struct ErrorBody { error: String }

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ErrorBody { error: msg.into() })).into_response()
}

fn err_chain(e: &dyn std::error::Error) -> String {
    let mut s = format!("{}", e);
    let mut cur = e.source();
    while let Some(src) = cur { s.push_str(&format!(" <- {}", src)); cur = src.source(); }
    s
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0f64;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

fn init_db(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS searches(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            query TEXT NOT NULL, lat REAL, lon REAL, radius_m INTEGER,
            result_count INTEGER, created_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS places_cache(
            place_key TEXT PRIMARY KEY, name TEXT, lat REAL, lon REAL,
            cuisine TEXT, data_json TEXT, fetched_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS favorites(
            place_key TEXT PRIMARY KEY, name TEXT, created_at TEXT NOT NULL);",
    ).expect("init db");
}

async fn api_search(State(state): State<AppState>, Query(p): Query<SearchParams>) -> Response {
    let q = match p.q { Some(ref s) if !s.trim().is_empty() => s.trim().to_string(), _ => return err(StatusCode::BAD_REQUEST, "Missing q - type a crossroads, e.g. 7th St & Bell Rd, Phoenix, AZ") };
    let radius = p.radius.unwrap_or(1200).clamp(100, 5000);

    // Geocode via Nominatim
    let geo_url = "https://nominatim.openstreetmap.org/search";
    let geo_resp = state.client.get(geo_url)
        .query(&[("format", "json"), ("limit", "1"), ("q", q.as_str())])
        .header("User-Agent", UA).header("Accept", "application/json")
        .send().await;
    let geo_json: serde_json::Value = match geo_resp {
        Ok(r) if r.status().is_success() => match r.json().await { Ok(v) => v, Err(e) => return err(StatusCode::BAD_GATEWAY, format!("Nominatim returned unreadable data: {e}")) },
        Ok(r) => return err(StatusCode::BAD_GATEWAY, format!("Nominatim failed with HTTP {}", r.status())),
        Err(e) => return err(StatusCode::BAD_GATEWAY, format!("Could not reach Nominatim: {}", err_chain(&e))),
    };
    let first = match geo_json.as_array().and_then(|a| a.first().cloned()) {
        Some(v) => v, None => return err(StatusCode::NOT_FOUND, format!("Could not find that crossroads: \"{q}\". Try adding a city and state.")),
    };
    let lat: f64 = first.get("lat").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let lon: f64 = first.get("lon").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let resolved_name = first.get("display_name").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Overpass
    let overpass_query = format!(
        "[out:json][timeout:25];(\n node[\"amenity\"~\"^(restaurant|fast_food|cafe|food_court|ice_cream|bar|pub)$\"](around:{radius},{lat},{lon});\n way[\"amenity\"~\"^(restaurant|fast_food|cafe|food_court|ice_cream|bar|pub)$\"](around:{radius},{lat},{lon});\n node[\"shop\"=\"bakery\"](around:{radius},{lat},{lon});\n way[\"shop\"=\"bakery\"](around:{radius},{lat},{lon});\n);out center tags;"
    );
    // Overpass can be slow / busy: try the main instance, then a public mirror.
    let mut op_json: Option<serde_json::Value> = None;
    let mut op_last_err = String::new();
    for endpoint in ["https://overpass-api.de/api/interpreter", "https://overpass.kumi.systems/api/interpreter"] {
        let op_resp = state.client.post(endpoint)
            .header("User-Agent", UA).header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[("data", overpass_query.as_str())]).send().await;
        match op_resp {
            Ok(r) if r.status().is_success() => match r.json().await {
                Ok(v) => { op_json = Some(v); break; },
                Err(e) => op_last_err = format!("Overpass ({endpoint}) returned unreadable data: {e}"),
            },
            Ok(r) => op_last_err = format!("Overpass ({endpoint}) failed with HTTP {}", r.status()),
            Err(e) => op_last_err = format!("Could not reach Overpass ({endpoint}): {}", err_chain(&e)),
        }
    }
    let op_json: serde_json::Value = match op_json {
        Some(v) => v,
        None => return err(StatusCode::BAD_GATEWAY, format!("{op_last_err} - Overpass can be busy, try again in a moment.")),
    };

    let mut places: Vec<Place> = Vec::new();
    if let Some(elements) = op_json.get("elements").and_then(|v| v.as_array()) {
        for el in elements {
            let tags = match el.get("tags") { Some(t) => t, None => continue };
            let name = match tags.get("name").and_then(|v| v.as_str()) { Some(n) if !n.trim().is_empty() => n.to_string(), _ => continue };
            let (plat, plon) = if let (Some(a), Some(o)) = (el.get("lat").and_then(|v| v.as_f64()), el.get("lon").and_then(|v| v.as_f64())) { (a, o) }
                else if let Some(c) = el.get("center") { (c.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0), c.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0)) }
                else { continue };
            let etype = el.get("type").and_then(|v| v.as_str()).unwrap_or("node");
            let eid = el.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let place_key = format!("{}-{}", etype, eid);
            let category = tags.get("amenity").and_then(|v| v.as_str()).map(|s| s.to_string())
                .or_else(|| tags.get("shop").and_then(|v| v.as_str()).map(|s| s.to_string())).unwrap_or_else(|| "food".into());
            let get_tag = |k: &str| tags.get(k).and_then(|v| v.as_str()).map(|s| s.to_string()).filter(|s| !s.trim().is_empty());
            let website = get_tag("website").or_else(|| get_tag("contact:website")).or_else(|| get_tag("url"));
            // Address from addr:* tags
            let mut addr_parts: Vec<String> = Vec::new();
            if let Some(n) = get_tag("addr:housenumber") { addr_parts.push(n); }
            if let Some(s) = get_tag("addr:street") { addr_parts.push(s); }
            if let Some(c) = get_tag("addr:city") { addr_parts.push(c); }
            if let Some(pc) = get_tag("addr:postcode") { addr_parts.push(pc); }
            let address = if addr_parts.is_empty() { None } else { Some(addr_parts.join(" ")) };
            let mut diet: Vec<String> = Vec::new();
            if let Some(obj) = tags.as_object() {
                for (k, v) in obj { if k.starts_with("diet:") && v.as_str() == Some("yes") { diet.push(k.trim_start_matches("diet:").to_string()); } }
            }
            diet.sort();
            let menu_search_url = format!("https://www.google.com/search?q={}", url_encode(&format!("{} menu", name)));
            let deals_search_url = format!("https://www.google.com/search?q={}", url_encode(&format!("{} deals coupons", name)));
            places.push(Place {
                place_key, name: name.clone(), lat: plat, lon: plon,
                distance_m: haversine_m(lat, lon, plat, plon).round() as i64,
                category, cuisine: get_tag("cuisine"), opening_hours: get_tag("opening_hours"),
                website: website.clone(), phone: get_tag("phone").or_else(|| get_tag("contact:phone")),
                address, branch: get_tag("branch"), diet,
                menu_url: website, menu_search_url, deals_search_url,
            });
        }
    }
    // Dedupe by place_key, sort by distance
    let mut seen = std::collections::HashSet::new();
    places.retain(|p| seen.insert(p.place_key.clone()));
    places.sort_by_key(|p| p.distance_m);

    // Log search + cache places in SQLite
    let now = Utc::now().to_rfc3339();
    if let Ok(db) = state.db.lock() {
        let _ = db.execute("INSERT INTO searches(query,lat,lon,radius_m,result_count,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![q, lat, lon, radius as i64, places.len() as i64, now]);
        for pl in &places {
            if let Ok(js) = serde_json::to_string(pl) {
                let _ = db.execute("INSERT OR REPLACE INTO places_cache(place_key,name,lat,lon,cuisine,data_json,fetched_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    rusqlite::params![pl.place_key, pl.name, pl.lat, pl.lon, pl.cuisine, js, now]);
            }
        }
    }

    Json(SearchResponse { query: q, resolved_name, lat, lon, radius_m: radius, count: places.len(), places }).into_response()
}

async fn api_history(State(state): State<AppState>) -> Response {
    let db = match state.db.lock() { Ok(d) => d, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "DB lock failed") };
    let mut stmt = match db.prepare("SELECT id,query,lat,lon,radius_m,result_count,created_at FROM searches ORDER BY id DESC LIMIT 50") { Ok(s) => s, Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()) };
    let rows = stmt.query_map([], |r| {
        Ok(serde_json::json!({"id": r.get::<_, i64>(0)?, "query": r.get::<_, String>(1)?, "lat": r.get::<_, Option<f64>>(2)?, "lon": r.get::<_, Option<f64>>(3)?, "radius_m": r.get::<_, Option<i64>>(4)?, "result_count": r.get::<_, Option<i64>>(5)?, "created_at": r.get::<_, String>(6)?}))
    });
    let mut out = Vec::new();
    if let Ok(rows) = rows { for row in rows.flatten() { out.push(row); } }
    Json(serde_json::json!({ "searches": out })).into_response()
}

async fn api_favorites_list(State(state): State<AppState>) -> Response {
    let db = match state.db.lock() { Ok(d) => d, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "DB lock failed") };
    let mut stmt = match db.prepare("SELECT place_key,name,created_at FROM favorites ORDER BY created_at DESC") { Ok(s) => s, Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()) };
    let rows = stmt.query_map([], |r| Ok(serde_json::json!({"place_key": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?, "created_at": r.get::<_, String>(2)?})));
    let mut out = Vec::new();
    if let Ok(rows) = rows { for row in rows.flatten() { out.push(row); } }
    Json(serde_json::json!({ "favorites": out })).into_response()
}

#[derive(Deserialize)]
struct FavBody { place_key: String, name: String }

async fn api_favorites_add(State(state): State<AppState>, Json(body): Json<FavBody>) -> Response {
    let now = Utc::now().to_rfc3339();
    let db = match state.db.lock() { Ok(d) => d, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "DB lock failed") };
    match db.execute("INSERT OR REPLACE INTO favorites(place_key,name,created_at) VALUES(?1,?2,?3)", rusqlite::params![body.place_key, body.name, now]) {
        Ok(_) => Json(serde_json::json!({"ok": true, "place_key": body.place_key})).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn api_favorites_delete(State(state): State<AppState>, Path(place_key): Path<String>) -> Response {
    let db = match state.db.lock() { Ok(d) => d, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "DB lock failed") };
    match db.execute("DELETE FROM favorites WHERE place_key=?1", rusqlite::params![place_key]) {
        Ok(_) => Json(serde_json::json!({"ok": true, "place_key": place_key})).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn health() -> impl IntoResponse { Json(serde_json::json!({"ok": true, "app": "crossroads"})) }

#[tokio::main]
async fn main() {
    let db_path = std::env::var("CROSSROADS_DB").unwrap_or_else(|_| "crossroads.db".to_string());
    let conn = Connection::open(&db_path).expect("open sqlite db");
    init_db(&conn);
    let client = reqwest::Client::builder().user_agent(UA).timeout(std::time::Duration::from_secs(120)).build().expect("http client");
    let state = AppState { db: Arc::new(Mutex::new(conn)), client };
    let host: String = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port: u16 = std::env::var("PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8891);
    let app = Router::new()
        .route("/api/search", get(api_search))
        .route("/api/history", get(api_history))
        .route("/api/favorites", get(api_favorites_list).post(api_favorites_add))
        .route("/api/favorites/:place_key", delete(api_favorites_delete))
        .route("/api/health", get(health))
        .fallback_service(ServeDir::new("static").append_index_html_on_directories(true))
        .with_state(state);
    let addr = format!("{}:{}", host, port);
    println!("Crossroads listening on http://{}  (db: {})", addr, db_path);
    println!("Open it via http://localhost:{port}, this machine's LAN IP, or its Tailscale IP (e.g. http://100.85.201.50:{port} on the i7).");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}

// keep BTreeMap import used for potential tag maps in future extensions
#[allow(dead_code)]
type TagMap = BTreeMap<String, String>;
