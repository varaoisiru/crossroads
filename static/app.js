// Crossroads frontend — vanilla JS. Search, filters, favorites, history, parallax.
const $ = (s) => document.querySelector(s);
const state = { radius: 1200, sort: "distance", cuisine: "all", places: [], favorites: new Set(), lastQuery: "" };

/* ---------- parallax: scroll + mousemove, multi-layer ---------- */
const layers = [...document.querySelectorAll("[data-parallax]")];
let mouseX = 0, mouseY = 0, ticking = false;
function applyParallax() {
  const y = window.scrollY;
  for (const el of layers) {
    const f = parseFloat(el.dataset.parallax) || 0;
    el.style.transform = `translate3d(${mouseX * f * 0.4}px, ${-y * f * 0.012 + mouseY * f * 0.4}px, 0)`;
  }
  ticking = false;
}
window.addEventListener("scroll", () => { if (!ticking) { ticking = true; requestAnimationFrame(applyParallax); } }, { passive: true });
window.addEventListener("mousemove", (e) => {
  mouseX = (e.clientX / window.innerWidth - 0.5);
  mouseY = (e.clientY / window.innerHeight - 0.5);
  if (!ticking) { ticking = true; requestAnimationFrame(applyParallax); }
}, { passive: true });

/* ---------- scroll reveal ---------- */
const io = new IntersectionObserver((entries) => {
  for (const en of entries) if (en.isIntersecting) { en.target.classList.add("in"); io.unobserve(en.target); }
}, { threshold: 0.08 });
function observeReveals(scope) { scope.querySelectorAll(".reveal:not(.in)").forEach((el) => io.observe(el)); }

/* ---------- animated count ---------- */
function animateCount(el, to) {
  const from = parseInt(el.textContent.replace(/\D/g, ""), 10) || 0;
  const t0 = performance.now(), dur = 650;
  (function step(t) {
    const p = Math.min(1, (t - t0) / dur), e = 1 - Math.pow(1 - p, 3);
    el.textContent = Math.round(from + (to - from) * e);
    if (p < 1) requestAnimationFrame(step);
  })(t0);
}

/* ---------- favorites ---------- */
async function loadFavorites() {
  try {
    const r = await fetch("/api/favorites"); const j = await r.json();
    state.favorites = new Set((j.favorites || []).map((f) => f.place_key));
    $("#favCount").textContent = state.favorites.size ? `(${state.favorites.size})` : "";
    document.querySelectorAll(".fav").forEach((b) => b.classList.toggle("on", state.favorites.has(b.dataset.key)));
  } catch (e) { /* favorites are optional if API hiccups */ }
}
async function toggleFav(btn) {
  const key = btn.dataset.key, name = btn.dataset.name;
  if (state.favorites.has(key)) {
    await fetch("/api/favorites/" + encodeURIComponent(key), { method: "DELETE" });
    state.favorites.delete(key);
  } else {
    await fetch("/api/favorites", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ place_key: key, name }) });
    state.favorites.add(key);
  }
  btn.classList.toggle("on", state.favorites.has(key));
  $("#favCount").textContent = state.favorites.size ? `(${state.favorites.size})` : "";
}

/* ---------- rendering ---------- */
function esc(s) { const d = document.createElement("div"); d.textContent = s ?? ""; return d.innerHTML; }
function fmtDist(m) { return m < 1000 ? `${m} m` : `${(m / 1000).toFixed(1)} km`; }

function filtered() {
  let list = [...state.places];
  if (state.cuisine !== "all") list = list.filter((p) => (p.cuisine || "Other") === state.cuisine);
  list.sort(state.sort === "name" ? (a, b) => a.name.localeCompare(b.name) : (a, b) => a.distance_m - b.distance_m);
  return list;
}

function renderChips() {
  const counts = new Map();
  for (const p of state.places) { const c = p.cuisine || "Other"; counts.set(c, (counts.get(c) || 0) + 1); }
  const chips = [["all", state.places.length], ...[...counts.entries()].sort((a, b) => b[1] - a[1])];
  $("#chips").innerHTML = chips.map(([c, n]) =>
    `<button type="button" class="chip ${state.cuisine === c ? "active" : ""}" data-cuisine="${esc(c)}">${esc(c)} · ${n}</button>`).join("");
  $("#chips").querySelectorAll(".chip").forEach((ch) => ch.addEventListener("click", () => { state.cuisine = ch.dataset.cuisine; renderChips(); renderResults(); }));
}

function renderResults() {
  const list = filtered();
  const box = $("#results");
  box.innerHTML = list.map((p) => {
    const hours = p.opening_hours ? `<div>◷ ${esc(p.opening_hours)} <span title="Opening hours come from OpenStreetMap tags; a place with no hours listed is unknown, not closed.">(hours from OSM — no hours shown means unknown, not closed)</span></div>` : `<div>◷ Hours unknown in OpenStreetMap</div>`;
    const addr = p.address ? `<div>⌖ ${esc(p.address)}</div>` : "";
    const phone = p.phone ? `<div>☎ ${esc(p.phone)}</div>` : "";
    const diet = p.diet && p.diet.length ? `<div>🥗 ${p.diet.map(esc).join(", ")}</div>` : "";
    const branch = p.branch ? ` · ${esc(p.branch)}` : "";
    const menuLink = p.menu_url
      ? `<a class="primary" href="${esc(p.menu_url)}" target="_blank" rel="noopener">Menu / Website ↗</a>`
      : `<a class="primary" href="${esc(p.menu_search_url)}" target="_blank" rel="noopener">Find menu ↗</a>`;
    return `<article class="card reveal" data-tilt>
      <button class="fav ${state.favorites.has(p.place_key) ? "on" : ""}" data-key="${esc(p.place_key)}" data-name="${esc(p.name)}" aria-label="Save favorite">♥</button>
      <h3>${esc(p.name)}</h3>
      <div class="meta">
        <span class="badge">${esc(p.category.replace(/_/g, " "))}</span>${p.cuisine ? `<span class="badge cuisine">${esc(p.cuisine)}</span>` : ""}
        <span class="dist">${fmtDist(p.distance_m)} away</span>${branch}
        ${addr}${hours}${phone}${diet}
      </div>
      <div class="links">
        ${menuLink}
        <a href="${esc(p.menu_search_url)}" target="_blank" rel="noopener">“${esc(p.name)} menu” search</a>
        <a class="deals" href="${esc(p.deals_search_url)}" target="_blank" rel="noopener">Deals &amp; coupons ↗</a>
      </div>
    </article>`;
  }).join("") || `<p class="meta">No places match that cuisine filter.</p>`;
  box.querySelectorAll(".fav").forEach((b) => b.addEventListener("click", () => toggleFav(b)));
  box.querySelectorAll("[data-tilt]").forEach((card) => {
    card.addEventListener("mousemove", (e) => {
      const r = card.getBoundingClientRect();
      const x = (e.clientX - r.left) / r.width, y = (e.clientY - r.top) / r.height;
      card.style.setProperty("--mx", `${x * 100}%`); card.style.setProperty("--my", `${y * 100}%`);
      card.style.transform = `perspective(800px) rotateY(${(x - 0.5) * 5}deg) rotateX(${(0.5 - y) * 5}deg) translateY(-2px)`;
    });
    card.addEventListener("mouseleave", () => { card.style.transform = ""; });
  });
  observeReveals(box);
}

/* ---------- search ---------- */
async function doSearch(q) {
  const status = $("#status");
  $("#skeleton").classList.remove("hidden"); $("#results").innerHTML = ""; $("#chips").innerHTML = "";
  status.textContent = "Geocoding your crossroads and asking OpenStreetMap…";
  $("#goBtn").disabled = true;
  try {
    const r = await fetch(`/api/search?q=${encodeURIComponent(q)}&radius=${state.radius}`);
    const j = await r.json();
    if (!r.ok) throw new Error(j.error || `Search failed (HTTP ${r.status})`);
    state.places = j.places || []; state.lastQuery = q; state.cuisine = "all";
    status.innerHTML = `<span class="count" id="resCount">0</span> food places within ${j.radius_m} m of <strong>${esc(q)}</strong>` +
      (j.resolved_name ? `<br><span class="meta">Resolved to: ${esc(j.resolved_name)}</span>` : "");
    animateCount($("#resCount"), j.count);
    animateCount($("#heroCount"), j.count);
    renderChips(); renderResults();
  } catch (e) {
    status.innerHTML = `<span class="err">⚠ ${esc(e.message)}<br>Nothing was invented to fill the gap — check the spelling, add a city/state, or try again in a moment (Overpass can be busy).</span>`;
  } finally {
    $("#skeleton").classList.add("hidden"); $("#goBtn").disabled = false;
  }
}

$("#searchForm").addEventListener("submit", (e) => {
  e.preventDefault();
  const q = $("#q").value.trim();
  if (q) doSearch(q);
  document.getElementById("searchShell").scrollIntoView({ behavior: "smooth", block: "start" });
});
document.querySelectorAll(".pill[data-radius]").forEach((b) => b.addEventListener("click", () => {
  document.querySelectorAll(".pill[data-radius]").forEach((x) => x.classList.remove("active"));
  b.classList.add("active"); state.radius = parseInt(b.dataset.radius, 10);
  if (state.lastQuery) doSearch(state.lastQuery);
}));
document.querySelectorAll(".sortpill").forEach((b) => b.addEventListener("click", () => {
  document.querySelectorAll(".sortpill").forEach((x) => x.classList.remove("active"));
  b.classList.add("active"); state.sort = b.dataset.sort; if (state.places.length) renderResults();
}));

/* ---------- drawer: history + favorites ---------- */
const drawer = $("#drawer"), scrim = $("#scrim");
function openDrawer(title, html) { $("#drawerTitle").textContent = title; $("#drawerBody").innerHTML = html; drawer.classList.add("open"); drawer.setAttribute("aria-hidden", "false"); scrim.classList.remove("hidden"); }
function closeDrawer() { drawer.classList.remove("open"); drawer.setAttribute("aria-hidden", "true"); scrim.classList.add("hidden"); }
$("#drawerClose").addEventListener("click", closeDrawer); scrim.addEventListener("click", closeDrawer);

$("#historyBtn").addEventListener("click", async () => {
  openDrawer("History", "<p class='meta'>Loading…</p>");
  try {
    const j = await (await fetch("/api/history")).json();
    const items = (j.searches || []).map((s) =>
      `<div class="hist-item" data-q="${esc(s.query)}"><div class="hq">${esc(s.query)}</div><div class="hm">${s.result_count} places · ${s.radius_m} m · ${new Date(s.created_at).toLocaleString()}</div></div>`).join("");
    $("#drawerBody").innerHTML = items || "<p class='meta'>No searches yet.</p>";
    $("#drawerBody").querySelectorAll(".hist-item").forEach((el) => el.addEventListener("click", () => { $("#q").value = el.dataset.q; closeDrawer(); doSearch(el.dataset.q); }));
  } catch (e) { $("#drawerBody").innerHTML = `<p class='meta'>Could not load history: ${esc(e.message)}</p>`; }
});
$("#favsBtn").addEventListener("click", async () => {
  openDrawer("Favorites", "<p class='meta'>Loading…</p>");
  try {
    const j = await (await fetch("/api/favorites")).json();
    const items = (j.favorites || []).map((f) => `<div class="hist-item"><div class="hq">♥ ${esc(f.name)}</div><div class="hm">Saved ${new Date(f.created_at).toLocaleString()}</div></div>`).join("");
    $("#drawerBody").innerHTML = items || "<p class='meta'>No favorites yet — tap the heart on any place.</p>";
  } catch (e) { $("#drawerBody").innerHTML = `<p class='meta'>Could not load favorites: ${esc(e.message)}</p>`; }
});

observeReveals(document);
loadFavorites();
