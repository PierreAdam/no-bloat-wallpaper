// No framework, no bundler. Text from remote sources only ever goes through textContent.
// All wording lives in lang/*.js and is read with t("key").
const { invoke, convertFileSrc } = window.__TAURI__.core;

// Bing's daily image feed accepts these markets. Many regions share the same image.
const BING_MARKETS = [
  "en-US", "en-GB", "en-CA", "en-AU", "en-NZ", "en-IN", "en-ZA", "fr-FR", "fr-CA", "fr-BE", "fr-CH",
  "de-DE", "de-AT", "de-CH", "es-ES", "es-MX", "es-AR", "es-US", "it-IT", "pt-BR", "pt-PT", "nl-NL",
  "nl-BE", "sv-SE", "nb-NO", "da-DK", "fi-FI", "pl-PL", "ru-RU", "uk-UA", "tr-TR", "ar-XA", "he-IL",
  "ja-JP", "ko-KR", "zh-CN", "zh-TW", "zh-HK", "th-TH", "en-WW",
];

// Market names in the current language ("French, Canada"), built by the browser's own locale data.
function marketName(code) {
  const [lang, region] = code.split("-");
  try {
    const l = new Intl.DisplayNames([getLanguage()], { type: "language" }).of(lang);
    if (region === "WW") return `${l}, ${t("worldwide")}`;
    if (region === "XA") return l;
    return `${l}, ${new Intl.DisplayNames([getLanguage()], { type: "region" }).of(region)}`;
  } catch (_) {
    return code;
  }
}

// Built-in sources (Bing, NASA, Wikimedia, Webb) exist at most once; folders and JSON endpoints can repeat.
const KINDS = {
  "bing": { label: () => t("kindBing"), stem: "bing", fields: [] },
  "nasa-apod": { label: () => t("kindNasa"), stem: "nasa", info: "nasaInfo", fields: [] },
  "wikimedia-potd": { label: () => t("kindWikimedia"), stem: "wikimedia", info: "wikimediaInfo", fields: [] },
  "esa-webb": { label: () => t("kindWebb"), stem: "webb", info: "webbInfo", fields: [] },
  "folder": { label: () => t("kindFolder"), multiple: true, fields: [["path", "fFolderPath", ""], ["license", "fLicenseNote", ""]] },
  "json": {
    label: () => t("kindJson"),
    multiple: true,
    fields: [["url", "fUrl", ""], ["image_field", "fImageField", "url"], ["title_field", "fTitleField", "title"],
      ["thumb_field", "fThumbField", ""], ["license", "fLicense", ""]],
  },
};

// Static, trusted markup only.
const ICONS = {
  heart: '<path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z"/>',
  // Feather "settings" (MIT): a real gear.
  gear: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
  back: '<path d="M15 5l-7 7 7 7"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  moon: '<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/>',
  min: '<path d="M5 12h14"/>',
  max: '<rect x="5" y="5" width="14" height="14" rx="1"/>',
  close: '<path d="M6 6l12 12M18 6L6 18"/>',
  up: '<path d="M6 15l6-6 6 6"/>',
  down: '<path d="M6 9l6 6 6-6"/>',
  refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/>',
};
function icon(name) {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 24 24");
  s.innerHTML = ICONS[name];
  return s;
}

// The same page runs in two windows: the main window, and the small panel opened from the tray icon.
const IS_FLYOUT = location.hash === "#flyout";
// Memory the tray icon costs (Task Manager's figure, WebView2 processes included), shown with the setting.
// Measured: about 2-4 MB with only the icon; about 76 MB while a window is open, given back when it closes.
const TRAY_RAM = { idle: "4 MB", open: "80 MB" };

let cfg = null;
let timer = null;
let screen = null; // which screen of the main window is shown ("home", "favorites", "settings")
let settingsTab = "system";
let offersCache = null; // { at: ms, sig, data }
let forceOffers = false; // the next preview load asks the sources again instead of using the cache
let scheduleOn = false; // whether the OS scheduler has our task (from get_status)
let scheduleDirty = false; // a setting that shapes the scheduled tasks changed since they were registered

// ---- helpers ----
function h(tag, props = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === "class") el.className = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (v !== undefined && v !== null && v !== false) el.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat(Infinity)) if (c != null) el.append(c.nodeType ? c : document.createTextNode(c));
  return el;
}
let toastTimer;
function toast(msg, isError = false) {
  const el = document.getElementById("toast");
  el.textContent = msg;
  el.className = isError ? "error" : "";
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), isError ? 6000 : 2500);
}

// The engine sends a translation key with its built-in license texts and known errors;
// anything else (the user's own license notes, unexpected errors) is shown as is.
function licenseText(item) {
  return item.license_key ? t(item.license_key) : item.license;
}
function errorText(e) {
  if (e && e.code) return t(e.code);
  return (e && e.message) || String(e);
}

async function call(cmd, args, b) {
  if (b) b.disabled = true;
  try {
    return await invoke(cmd, args);
  } catch (e) {
    toast(errorText(e), true);
    throw e;
  } finally {
    if (b) b.disabled = false;
  }
}
function pill(label, onclick, cls = "") {
  return h("button", { class: `pill ${cls}`, onclick: (e) => onclick(e.currentTarget) }, label);
}
function iconPill(name, label, onclick) {
  return h("button", { class: "pill icon", "aria-label": label, onclick: (e) => onclick(e.currentTarget) }, icon(name));
}
function toggle(on, onchange, label) {
  const b = h("button", { class: "sw", role: "switch", "aria-checked": String(on), "aria-label": label });
  b.addEventListener("click", () => {
    const next = b.getAttribute("aria-checked") !== "true";
    b.setAttribute("aria-checked", String(next));
    onchange(next);
  });
  return b;
}

// Settings are saved shortly after each change; leaving the screen flushes immediately.
let saveTimer = null;
function queueSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(flushSave, 600);
}
async function flushSave() {
  if (saveTimer == null) return;
  clearTimeout(saveTimer);
  saveTimer = null;
  offersCache = null; // sources may have changed
  try {
    await invoke("save_config", { cfg });
    // Keep the registered tasks in line with the settings (interval, login check, battery).
    if (scheduleDirty && scheduleOn) {
      scheduleDirty = false;
      await invoke("install_schedule");
    }
  } catch (e) { toast(errorText(e), true); }
}
function queueScheduleSave() {
  scheduleDirty = true;
  queueSave();
}

// ---- theme (system by default, manual override remembered) ----
function isDark() {
  const th = document.documentElement.dataset.theme;
  return th ? th === "dark" : matchMedia("(prefers-color-scheme: dark)").matches;
}
function themeButton() {
  const b = h("button", { class: "pill icon" });
  const paint = () => {
    // Show where a click goes: the moon switches to dark, the sun back to light.
    b.replaceChildren(icon(isDark() ? "sun" : "moon"));
    b.setAttribute("aria-label", isDark() ? t("themeToLight") : t("themeToDark"));
  };
  b.addEventListener("click", () => {
    const next = isDark() ? "light" : "dark";
    document.documentElement.dataset.theme = next;
    try { localStorage.setItem("nbw-theme", next); } catch (_) {}
    paint();
  });
  paint();
  return b;
}

// ---- custom title bar (native decorations are off) ----
function titleBar() {
  const win = window.__TAURI__.window.getCurrentWindow();
  const ctl = (name, label, fn, cls = "") =>
    h("button", { class: `wc ${cls}`, "aria-label": label, onclick: fn }, icon(name));
  document.getElementById("titlebar").replaceChildren(
    h("span", { class: "ttl", "data-tauri-drag-region": "" }, "No Bloat Wallpaper"),
    h("div", { class: "wcs" },
      ctl("min", "Minimize", () => win.minimize()),
      ctl("max", "Maximize", () => win.toggleMaximize()),
      ctl("close", "Close", () => win.close(), "close"))
  );
}

// Every screen: a scrolling body, then a fixed footer (disclaimer above the buttons).
function show(body, foot) {
  clearInterval(timer);
  document.getElementById("app").replaceChildren(
    h("div", { class: "scroll" }, body),
    h("div", { class: "foot" }, foot, h("div", { class: "disclaimer" }, t("disclaimer")))
  );
}

// ---- Home ----
function formatRemaining(sec) {
  const m = Math.max(1, Math.ceil(sec / 60));
  return m >= 60 ? `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")} min` : `${m} min`;
}

// The next change comes from the OS scheduler's own registration (start time + repeat interval).
function nextChange(st) {
  const s = st.schedule;
  if (!s) return null;
  const hours = s.interval_hours || st.interval_hours;
  const period = hours * 3600 * 1000;
  const start = new Date(s.start).getTime(); // no timezone in the string: parsed as local time
  if (!Number.isFinite(start)) return { text: t("scheduledShort"), detail: "" };
  const now = Date.now();
  const k = now <= start ? 0 : Math.ceil((now - start) / period);
  const next = start + k * period;
  const remaining = Math.max(0, Math.round((next - now) / 1000));
  const at = new Date(next).toLocaleTimeString(getLanguage(), { hour: "2-digit", minute: "2-digit" });
  return {
    text: remaining === 0 ? t("dueNow") : formatRemaining(remaining),
    detail: t("nextAt", { at, hours }) + (st.login_check ? t("alsoChecksLogin", { secs: cfg.login_delay_secs }) : ""),
  };
}

function friendlyError(group) {
  return group.error_code ? t(group.error_code) : t("couldNotLoad", { msg: group.error });
}

// One horizontal strip of previews per enabled source.
function offersSection(group, cur) {
  const src = cfg.sources.find((s) => s.id === group.id);
  const title = h("div", { class: "mut small sec" }, group.id, src ? ` · ${KINDS[src.kind]?.label() ?? src.kind}` : "");
  if (group.error) return h("div", {}, title, h("div", { class: "card mut small" }, friendlyError(group)));
  if (!group.offers.length) return h("div", {}, title, h("div", { class: "card mut small" }, t("noImages")));
  return h("div", {}, title,
    h("div", { class: "strip" }, group.offers.map((o) => {
      // Previews are always local files; without one the tile stays a plain placeholder.
      const img = h("img", { src: o.thumb ? convertFileSrc(o.thumb) : null, alt: "", loading: "lazy", class: o.thumb ? "" : "broken" });
      img.addEventListener("error", () => img.classList.add("broken"));
      return h("button", {
        class: "thumb-btn" + (cur && cur.id === o.id ? " active" : ""),
        title: o.title + (o.credit ? `\n${o.credit}` : ""),
        "aria-label": t("useImage", { title: o.title }),
        onclick: async (e) => {
          await call("apply_offer", { sourceId: group.id, offerId: o.id }, e.currentTarget);
          toast(t(cfg.rotation === "random" ? "nowUsing" : "nowUsingLatest", { title: o.title }));
          afterChange();
        },
      }, img);
    })));
}

// The picture in use right now: big image, title, credit, and the explanation when the source has one.
function heroCard(cur) {
  if (!cur) return h("div", { class: "card mut" }, t("noWallpaper"));
  const long = (cur.description || "").length > 220;
  const expl = cur.description ? h("p", { class: "expl" + (long ? " clamp" : "") }, cur.description) : null;
  const more = long
    ? h("button", { class: "linkbtn", onclick: (e) => {
        const open = expl.classList.toggle("clamp") === false;
        e.currentTarget.textContent = open ? t("showLess") : t("readMore");
      } }, t("readMore"))
    : null;
  return h("div", { class: "card hero" },
    // A copy sized for the window (made by the engine), never the full-size picture.
    h("img", { class: "hero-img", src: cur.preview ? convertFileSrc(cur.preview) : null, alt: cur.title }),
    h("div", { class: "row top" },
      h("div", { class: "grow" },
        h("h2", { class: "hero-title" }, cur.title),
        cur.credit ? h("div", { class: "mut small" }, cur.credit) : null),
      h("button", {
        class: "ibtn", "aria-label": t("saveFav"), title: t("saveFav"),
        onclick: async () => { await call("favorite_current"); toast(t("savedFav")); },
      }, icon("heart"))),
    expl, more,
    h("div", { class: "mut small lic" }, t("license", { license: licenseText(cur) })));
}

async function home() {
  screen = "home";
  await flushSave();
  const cur = await call("get_current");
  const enabled = cfg.sources.filter((s) => s.enabled);

  const hint = () => h("div", { class: "row hintrow" },
    h("span", { class: "mut small grow" }, t(cfg.rotation === "random" ? "pickHint" : "pickHintLatest")),
    h("button", { class: "ibtn", "aria-label": t("refreshPreviews"), title: t("refreshPreviews"),
      onclick: () => { forceOffers = true; home(); } }, icon("refresh")));

  const previews = h("div", { class: "previews" });
  if (!enabled.length) {
    previews.append(h("div", { class: "card" },
      h("strong", {}, t("chooseSources")),
      h("p", { class: "mut small", style: "margin:4px 0 12px" }, t("noSourceEnabled")),
      pill(t("openSettings"), settings)));
  } else {
    previews.append(hint(), h("div", { class: "card mut small loading" }, t("loadingPreviews")));
  }

  const changeBtn = pill(t("changeNow"), async (b) => {
    b.textContent = t("fetching");
    try {
      const res = await call("run_now", {}, b);
      toast(res.changed ? t("wallpaperUpdated") : t("alreadyLatest"));
      offersCache = null;
      await home();
    }
    finally { b.textContent = t("changeNow"); }
  }, "primary");

  show([heroCard(cur), previews],
    h("div", { class: "actions" },
      changeBtn, pill(t("favorites"), favorites),
      h("span", { class: "grow" }), // pushes the two icon buttons to the right
      themeButton(), iconPill("gear", t("settings"), settings)));

  // Previews come from the cache when recent (the engine decides), so this is usually instant.
  if (enabled.length) {
    const sig = JSON.stringify(enabled.map((s) => [s.id, s.kind, s.market, s.path, s.url, s.image_field, s.title_field, s.thumb_field]));
    const fresh = !forceOffers && offersCache && offersCache.sig === sig && Date.now() - offersCache.at < 5 * 60 * 1000;
    try {
      const data = fresh ? offersCache.data : await invoke("list_offers", { force: forceOffers });
      forceOffers = false;
      offersCache = { at: Date.now(), sig, data };
      // Never show a source that is not currently enabled.
      const shown = data.filter((g) => enabled.some((s) => s.id === g.id));
      previews.replaceChildren(hint(), ...shown.map((g) => offersSection(g, cur)));
    } catch (e) {
      forceOffers = false;
      previews.replaceChildren(h("div", { class: "card mut small" }, t("couldNotLoadPreviews", { msg: errorText(e) })));
    }
  }
}

// ---- Favorites ----
function header(title) {
  return h("div", { class: "head" },
    h("button", { class: "ibtn", "aria-label": t("back"), onclick: home }, icon("back")),
    h("h1", {}, title));
}

async function favorites() {
  screen = "favorites";
  const favs = await call("list_favorites");
  show([
    header(t("favorites")),
    favs.length
      ? h("div", { class: "grid" }, favs.map((f) =>
          h("div", { class: "card" },
            h("img", { src: f.preview ? convertFileSrc(f.preview) : null, alt: "", loading: "lazy" }),
            h("h3", { class: "ell" }, f.title),
            h("p", { class: "ell", title: licenseText(f) }, licenseText(f)),
            h("div", { class: "row" },
              pill(t("set"), async (b) => { await call("apply_favorite", { id: f.id }, b); toast(t("applied")); }),
              pill(t("delete"), async (b) => { await call("delete_favorite", { id: f.id }, b); favorites(); }, "danger")))))
      : h("div", { class: "card mut" }, t("noFavorites")),
  ], null);
}

// ---- Settings ----
function sourceHelp(key) {
  return h("div", { class: "help" },
    h("p", { class: "mut small", style: "margin:0" }, t(key)));
}

// Re-render the settings screen without losing the scroll position.
function refreshSettings() {
  const y = document.querySelector(".scroll")?.scrollTop ?? 0;
  settings();
  const sc = document.querySelector(".scroll");
  if (sc) sc.scrollTop = y;
}

function moveSource(i, delta) {
  const j = i + delta;
  if (j < 0 || j >= cfg.sources.length) return;
  [cfg.sources[i], cfg.sources[j]] = [cfg.sources[j], cfg.sources[i]];
  queueSave();
  refreshSettings();
}

function sourceCard(src, i) {
  const spec = KINDS[src.kind];
  const order = h("div", { class: "order" },
    h("button", { class: "ibtn", "aria-label": t("moveUp"), title: t("moveUp"), disabled: i === 0, onclick: () => moveSource(i, -1) }, icon("up")),
    h("span", { class: "mut small num" }, String(i + 1)),
    h("button", { class: "ibtn", "aria-label": t("moveDown"), title: t("moveDown"), disabled: i === cfg.sources.length - 1, onclick: () => moveSource(i, 1) }, icon("down")));
  // A name must be non-empty and unique: an invalid one is flagged and not saved.
  const name = h("input", { type: "text", value: src.id, oninput: (e) => {
    const v = e.target.value.trim();
    const ok = v !== "" && !cfg.sources.some((o) => o !== src && o.id === v);
    name.classList.toggle("invalid", !ok);
    name.setAttribute("aria-invalid", String(!ok));
    name.title = ok ? "" : t("errNameInvalid");
    if (ok) { src.id = v; queueSave(); }
  } });
  const fields = h("div", { class: "fields" }, h("label", {}, t("fName")), name);

  if (src.kind === "bing") {
    const codes = !src.market || BING_MARKETS.includes(src.market) ? BING_MARKETS : [src.market, ...BING_MARKETS];
    fields.append(h("label", {}, t("fMarket")),
      h("select", { onchange: (e) => { src.market = e.target.value; queueSave(); } },
        codes.map((code) => h("option", { value: code, selected: code === (src.market || "en-US") }, `${marketName(code)} (${code})`))));
  }
  for (const [key, labelKey, placeholder] of spec?.fields ?? []) {
    fields.append(h("label", {}, t(labelKey)),
      h("input", { type: "text", value: src[key] ?? "", placeholder,
        oninput: (e) => { src[key] = e.target.value || null; queueSave(); } }));
  }

  return h("div", { class: "card" },
    h("div", { class: "row" },
      order,
      toggle(src.enabled, (on) => { src.enabled = on; queueSave(); }, t("useSource", { name: src.id })),
      h("strong", { class: "grow" }, spec?.label() ?? src.kind),
      pill(t("remove"), () => { cfg.sources.splice(i, 1); queueSave(); refreshSettings(); }, "danger")),
    fields,
    spec?.info ? sourceHelp(spec.info) : null);
}

function formatSize(bytes) {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / 1048576).toFixed(1)} MB`;
}

function languageCard() {
  const select = h("select", { onchange: (e) => {
      cfg.language = e.target.value;
      setLanguage(cfg.language);
      queueSave();
      refreshSettings(); // redraw in the new language
    } },
    h("option", { value: "auto", selected: !cfg.language || cfg.language === "auto" }, t("languageAuto")),
    NBW_LANGUAGES.map(([code, name]) => h("option", { value: code, selected: cfg.language === code }, name)));
  return h("div", { class: "card" },
    h("div", { class: "fields", style: "margin-top:0" }, h("label", {}, t("language")), select));
}

function cacheCard() {
  const size = h("span", { class: "mut small" }, "");
  const refreshSize = () => call("cache_size").then((n) => (size.textContent = t("cacheUsed", { size: formatSize(n) }))).catch(() => {});
  refreshSize();
  return h("div", { class: "card" },
    h("strong", {}, t("cache")),
    h("p", { class: "mut small", style: "margin:4px 0 0" }, t("cacheText", { minutes: cfg.cache_minutes })),
    h("div", { class: "row", style: "margin-top:10px" },
      pill(t("clearCache"), async (b) => { await call("clear_cache", {}, b); offersCache = null; toast(t("cacheCleared")); refreshSize(); }),
      size));
}

function settings() {
  screen = "settings";
  const hours = h("input", { type: "number", min: "1", max: "168", value: cfg.interval_hours,
    oninput: (e) => { cfg.interval_hours = Math.min(168, Math.max(1, parseInt(e.target.value) || 6)); queueScheduleSave(); } });
  const delay = h("input", { type: "number", min: "0", max: "600", value: cfg.login_delay_secs, disabled: !cfg.check_on_login,
    oninput: (e) => { cfg.login_delay_secs = Math.min(600, Math.max(0, parseInt(e.target.value) || 0)); queueScheduleSave(); } });
  // Laptops only: shown once the status says this computer has a battery.
  const batteryLabel = h("label", { style: "display:none" }, t("runOnBattery"));
  const batteryRow = h("div", { class: "row", style: "display:none" },
    toggle(cfg.run_on_battery, (on) => { cfg.run_on_battery = on; queueScheduleSave(); }, t("runOnBattery")),
    h("span", { class: "mut small" }, t("runOnBatteryHint")));
  const rotation = h("select", { onchange: (e) => { cfg.rotation = e.target.value; queueSave(); } },
    h("option", { value: "latest", selected: cfg.rotation !== "random" }, t("latestFirst")),
    h("option", { value: "random", selected: cfg.rotation === "random" }, t("randomUnseen")));
  const nextLine = h("p", { class: "next small" }, "");
  // Says plainly whether the OS scheduler has our task: without it, nothing changes on its own.
  const badge = h("span", { class: "badge", hidden: true }, "");
  call("get_status").then((st) => {
    scheduleOn = !!st.schedule;
    badge.textContent = t(scheduleOn ? "schedOn" : "schedOff");
    badge.className = `badge ${scheduleOn ? "on" : "off"}`;
    badge.hidden = false;
    if (st.has_battery) batteryLabel.style.display = batteryRow.style.display = "";
    const n = nextChange(st);
    nextLine.textContent = n ? t("nextChangeIn", { time: n.text, detail: n.detail }) : t("scheduleOff");
  }).catch(() => {});

  // A removed built-in source can be added back, but only once.
  const addable = Object.entries(KINDS).filter(([k, v]) => v.multiple || !cfg.sources.some((s) => s.kind === k));
  const kind = h("select", {}, addable.map(([k, v]) => h("option", { value: k }, v.label())));
  const add = pill(t("add"), () => {
    const k = kind.value;
    const stem = KINDS[k].stem ?? k;
    let id = stem;
    for (let n = 2; cfg.sources.some((s) => s.id === id); n++) id = `${stem}-${n}`;
    cfg.sources.push({ id, kind: k, enabled: true, market: k === "bing" ? "en-US" : null, path: null, url: null,
      image_field: k === "json" ? "url" : null, title_field: k === "json" ? "title" : null, thumb_field: null, license: null });
    queueSave();
    refreshSettings();
  });

  const schedulerCard = h("div", { class: "card" },
      h("div", { class: "row" }, h("strong", { class: "grow" }, t("scheduler")), badge),
      nextLine,
      h("p", { class: "mut small", style: "margin:4px 0 0" }, t("schedulerHelp")),
      h("div", { class: "fields" },
        h("label", {}, t("everyHours")), hours,
        h("label", {}, t("selection")), rotation,
        h("label", {}, t("startWithWindows")),
        h("div", { class: "row" },
          toggle(cfg.check_on_login, (on) => { cfg.check_on_login = on; delay.disabled = !on; queueScheduleSave(); }, t("checkAfterLogin")),
          h("span", { class: "mut small" }, t("catchUp"))),
        h("label", {}, t("waitSeconds")), delay,
        batteryLabel, batteryRow),
      h("div", { class: "row", style: "margin-top:12px" },
        pill(t("saveAndTurnOn"), async (b) => {
          await flushSave();
          const n = await call("install_schedule", {}, b);
          scheduleOn = true;
          scheduleDirty = false;
          toast(t("scheduledEvery", { n }));
          refreshSettings();
        }, "primary"),
        pill(t("turnOff"), async (b) => { await call("remove_schedule", {}, b); scheduleOn = false; toast(t("scheduleRemoved")); refreshSettings(); })));

  const body = settingsTab === "system"
    ? [schedulerCard, trayCard(), languageCard(), cacheCard()]
    : [h("div", { class: "mut small sec" }, t("sourcesHint")),
      cfg.sources.map(sourceCard),
      h("div", { class: "card row" }, h("div", { class: "grow" }, kind), add)];
  show([header(t("settings")), settingsTabs(), body], null);
}

// System (how the app runs, starting with whether the schedule is on) and Sources (where the
// pictures come from), so neither screen is crowded.
function settingsTabs() {
  const tab = (id, label) => h("button", {
    class: "tab", role: "tab", "aria-selected": String(settingsTab === id),
    onclick: () => { if (settingsTab !== id) { settingsTab = id; settings(); } },
  }, label);
  return h("div", { class: "tabs", role: "tablist" }, tab("system", t("tabSystem")), tab("sources", t("tabSources")));
}

// Optional tray icon. Off by default; turning it on keeps a small process running, which the text states plainly.
function trayCard() {
  return h("div", { class: "card" },
    h("div", { class: "row" },
      h("strong", { class: "grow" }, t("tray")),
      toggle(cfg.tray, async (on) => {
        cfg.tray = on;
        queueSave();
        await flushSave();
        toast(t(on ? "trayOnToast" : "trayOffToast"));
      }, t("tray"))),
    h("p", { class: "mut small", style: "margin:4px 0 0" }, t("trayText", TRAY_RAM)));
}

// ---- Tray flyout: the current picture, quick actions and the previews ----
async function flyout() {
  const cur = await call("get_current");
  const enabled = cfg.sources.filter((s) => s.enabled);
  const previews = h("div", { class: "previews" },
    enabled.length ? h("div", { class: "card mut small loading" }, t("loadingPreviews")) : h("div", { class: "card mut small" }, t("noSourceEnabled")));
  const changeBtn = pill(t("changeNow"), async (b) => {
    b.textContent = t("fetching");
    try {
      const res = await call("run_now", {}, b);
      toast(res.changed ? t("wallpaperUpdated") : t("alreadyLatest"));
      await afterChange();
    } finally { b.textContent = t("changeNow"); }
  }, "primary");
  show([heroCard(cur), previews],
    h("div", { class: "actions" }, changeBtn, h("span", { class: "grow" }),
      iconPill("gear", t("openSettings"), () => invoke("open_main_window"))));
  if (!enabled.length) return;
  try {
    const data = await invoke("list_offers", { force: false });
    const shown = data.filter((g) => enabled.some((s) => s.id === g.id));
    previews.replaceChildren(...shown.map((g) => offersSection(g, cur)));
  } catch (e) {
    previews.replaceChildren(h("div", { class: "card mut small" }, t("couldNotLoadPreviews", { msg: errorText(e) })));
  }
}

// After the wallpaper changed: redraw, and tell the main window when the change came from the flyout.
async function afterChange() {
  if (IS_FLYOUT) {
    window.__TAURI__.event.emitTo("main", "nbw:changed").catch(() => {});
    await flyout();
  } else {
    await home();
  }
}

// The settings file cannot be read (edited by hand, damaged): say so, show why, and offer to
// start again from the defaults (the engine keeps the old file as a copy).
function brokenConfig(e) {
  setLanguage("auto");
  show(h("div", { class: "card" },
    h("strong", {}, t("errConfigInvalid")),
    h("p", { class: "mut small", style: "margin:8px 0 12px;overflow-wrap:anywhere" }, (e && e.message) || String(e)),
    pill(t("resetConfig"), async (b) => {
      const backup = await call("reset_config", {}, b);
      toast(t("configResetDone", { path: backup }));
      start();
    }, "primary")), null);
}

async function start() {
  try {
    cfg = await invoke("get_config");
  } catch (e) {
    if (e && e.code === "errConfigInvalid") return brokenConfig(e);
    toast(errorText(e), true);
    throw e;
  }
  setLanguage(cfg.language);
  await (IS_FLYOUT ? flyout() : home());
}

(async () => {
  if (IS_FLYOUT) {
    document.body.classList.add("flyout");
    addEventListener("keydown", (e) => { if (e.key === "Escape") window.__TAURI__.window.getCurrentWindow().close(); });
  } else {
    titleBar();
    // A change made from the tray flyout: refresh the home screen if it is showing.
    window.__TAURI__.event.listen("nbw:changed", () => { if (screen === "home") home(); });
  }
  await start();
})();
