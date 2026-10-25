# Crossroads

**Type a crossroads. See every food place around it — with menus and deals links.**

Crossroads is a locally-hosted web app in the spirit of UltraSync: one text field for an
intersection (e.g. `7th St & Bell Rd, Phoenix, AZ`), and it shows all the restaurants,
fast food, cafés, food courts, ice cream, bars, pubs and bakeries near that corner —
each with its cuisine, distance, opening hours (when known), address, phone, a
menu/website link, and clearly-labelled search links for its menu and its deals/coupons.

## Stack

- **Rust backend** — Axum + tokio + reqwest (rustls TLS) + serde/serde_json.
- **SQLite** — via `rusqlite` with the `bundled` feature. That is the **SQLite C library
  compiled and bound directly into the Rust binary** — no separate database server,
  no system SQLite install needed. The database file is `crossroads.db`, created
  automatically on first run.
- **Advanced CSS frontend** — vanilla JS + a hand-written stylesheet with:
  multi-layer parallax hero (scroll *and* mousemove, CSS `transform` + `perspective`),
  liquid-glass panels (`backdrop-filter` blur/saturate, translucent fills, hairline
  borders — the same Liquid Glass language as UltraSync), a sticky glass search bar,
  scroll-driven reveal animations (`IntersectionObserver`), an animated gradient +
  noise + grid background, 3D card hover tilt with a light-tracking highlight,
  skeleton loading shimmer, animated result counts, and a responsive mobile-first
  dark layout.

## Run it

```bash
# Install Rust once (if needed): https://rustup.rs
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

cargo run --release
# → Crossroads listening on http://0.0.0.0:8891
```

The server **binds to `0.0.0.0` by default** (same pattern as Justin's other apps), so it
is reachable from other devices — not just localhost. Open it any of these ways:

- On the machine running it: <http://localhost:8891>
- From another device on the same network: `http://<that machine's LAN IP>:8891`
- Over Tailscale (by IP, not DNS): `http://<that machine's Tailscale IP>:8891` —
  e.g. `http://100.85.201.50:8891` when it runs on the i7.

Host and port are both env-overridable:

```bash
HOST=0.0.0.0 PORT=9000 cargo run --release
```

> Note: binding `0.0.0.0` is what makes LAN/Tailscale access work when the app runs on
> your own machine. (The Muse VM this was built on is a Tailscale client with no
> inbound tailnet access, so inbound tailnet reachability was not — and cannot be —
> demoed from there; it follows from the bind when run on your box.)

The database path can be changed with `CROSSROADS_DB` (default `crossroads.db`).

## How it works / data sources

No API keys, no paid services:

1. **Geocoding** — the crossroads text is resolved with
   [Nominatim](https://nominatim.openstreetmap.org/search) (OpenStreetMap), sent with a
   descriptive `User-Agent`.
2. **Places** — [Overpass API](https://overpass-api.de/api/interpreter) returns
   (with an automatic fallback to the public `overpass.kumi.systems` mirror if the main
   instance is busy — Overpass responses for a busy city corner can take 30 s+)
   `amenity=restaurant|fast_food|cafe|food_court|ice_cream|bar|pub` and `shop=bakery`
   within the chosen radius (500 m / 1.2 km / 2.5 km). Name, cuisine, opening hours,
   website, phone, address parts, branch and `diet:*` tags come straight from
   OpenStreetMap tags. Distances are computed in Rust (haversine) and results are
   sorted nearest-first.
3. **SQLite** — every search is logged in `searches`, Overpass results are cached in
   `places_cache`, and your hearts are stored in `favorites`.

Tables:

| Table | Columns |
|---|---|
| `searches` | `id, query, lat, lon, radius_m, result_count, created_at` |
| `places_cache` | `place_key, name, lat, lon, cuisine, data_json, fetched_at` |
| `favorites` | `place_key, name, created_at` |

## Honesty note: menus & deals

Crossroads **never fabricates prices, menus, or deals**.

- If a place lists a website in OpenStreetMap, that real website is the
  **Menu / Website** link.
- Every place also gets two clearly-labelled **search** links — `"<name> menu"` and
  `"<name> deals coupons"` (Google search URLs) — so you can check the place's actual
  menu and any real coupons/offers yourself.
- The "Value tips" panel is generic advice only; it contains no invented offers.
- Opening hours come from OpenStreetMap tags. If a place has no hours listed, the UI
  says **unknown** — it does *not* guess "open" or "closed".
- If Nominatim or Overpass fail, the API returns a clean JSON error and the UI shows
  it honestly instead of inventing results.

## API

| Method | Path | What it does |
|---|---|---|
| `GET` | `/api/search?q=<crossroads>&radius=<500\|1200\|2500>` | Geocode + nearby food places (JSON) |
| `GET` | `/api/history` | Last 50 searches from SQLite |
| `GET` | `/api/favorites` | List favorites |
| `POST` | `/api/favorites` `{place_key, name}` | Save a favorite |
| `DELETE` | `/api/favorites/{place_key}` | Remove a favorite |
| `GET` | `/api/health` | `{ok: true, app: "crossroads"}` |
| `GET` | `/` | The web app (static frontend served by Axum) |

## Notes

- Please be kind to Nominatim/Overpass — they're free community services. This app is
  meant for personal, local use.
- Data: © OpenStreetMap contributors.
