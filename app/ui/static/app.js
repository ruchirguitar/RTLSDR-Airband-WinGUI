// Frequency Recorder - vanilla JS, no build step. Talks to the backend in app/backend.
"use strict";

const $ = (sel, el = document) => el.querySelector(sel);
const $$ = (sel, el = document) => Array.from(el.querySelectorAll(sel));
const esc = (v) => String(v ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const DAY_NAMES = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const ICON = {
  play: '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4l13 8-13 8z"/></svg>',
  pause: '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg>',
  headphones: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M3 14v-2a9 9 0 0 1 18 0v2"/><rect x="3" y="14" width="4" height="7" rx="1"/><rect x="17" y="14" width="4" height="7" rx="1"/></svg>',
  edit: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 20h4L19 9l-4-4L4 16z"/></svg>',
  folder: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
  trash: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></svg>',
  plus: '<svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>',
  alert: '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 3l10 18H2z"/><path d="M12 10v4M12 18h0"/></svg>',
  left: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M15 6l-6 6 6 6"/></svg>',
  download: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14"/></svg>',
  right: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M9 6l6 6-6 6"/></svg>',
};

let station = null;
/** Who this page is: the app's own window, or another device on the network (remote.rs). */
let client = { remote: false, read_only: false };
let ctcssTones = [];
let info = {};
let status = null;
let recent = [];
let recentLoaded = 0;

// ---------- small helpers ----------

async function api(path, opts = {}) {
  const init = { method: opts.method || "GET", headers: {} };
  if (opts.body !== undefined) {
    init.headers["content-type"] = "application/json";
    init.body = JSON.stringify(opts.body);
  }
  const res = await fetch(path, init);
  if (res.status === 401 && client.remote) { location.href = "/login"; throw new Error("Signed out"); }
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `${res.status} ${res.statusText}`);
  return data;
}

function fmtMhz(mhz) {
  const s = Number(mhz).toFixed(4);
  return s.endsWith("0") ? s.slice(0, -1) : s;
}
const label = (f) => (f.name && f.name.trim()) || `${fmtMhz(f.mhz)} MHz`;
// one colour per frequency (amber stays reserved for "on air")
const FREQ_COLOURS = ["#3dd6f5", "#e879f9", "#a3e635", "#60a5fa", "#f472b6", "#2dd4bf", "#c084fc", "#fb923c"];
const freqColour = (f) => FREQ_COLOURS[Math.max(0, station.frequencies.indexOf(f)) % FREQ_COLOURS.length];
const sameMhz = (a, b) => Math.abs(a - b) < 0.0005;
const clock = (unix) => new Date(unix * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
const clockSec = (unix) => new Date(unix * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });

function dur(sec) {
  if (sec == null || !isFinite(sec)) return "";
  sec = Math.max(0, Math.round(sec));
  const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}` : `${m}:${String(s).padStart(2, "0")}`;
}

function ago(unix) {
  if (!unix) return "Not heard yet";
  const s = Date.now() / 1000 - unix;
  if (s < 60) return "Heard just now";
  if (s < 3600) return `Heard ${Math.round(s / 60)} min ago`;
  if (s < 86400 * 2) return `Heard ${Math.round(s / 3600)} h ago`;
  return `Heard ${Math.round(s / 86400)} days ago`;
}

function uptime(unix) {
  const s = Date.now() / 1000 - unix;
  if (s < 60) return "just started";
  const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
  return d ? `up ${d} d ${h} h` : h ? `up ${h} h ${m} min` : `up ${m} min`;
}

function bytes(n) {
  if (n == null) return "…";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
  return `${n.toFixed(i && n < 10 ? 1 : 0)} ${u[i]}`;
}

function startOfDay(d) { const x = new Date(d); x.setHours(0, 0, 0, 0); return x; }
const unixOf = (d) => d.getTime() / 1000;
const dateInput = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
const encodePath = (p) => p.split("/").map(encodeURIComponent).join("/");

let toastTimer = null;
function toast(msg, isError = false) {
  let el = $(".toast");
  if (!el) { el = document.createElement("div"); el.className = "toast"; el.setAttribute("role", "status"); document.body.appendChild(el); }
  el.textContent = msg;
  el.classList.toggle("err", isError);
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.remove(), isError ? 6000 : 2500);
}

async function saveStation(next, okMsg = "Saved") {
  try {
    const res = await api("/api/station", { method: "PUT", body: next });
    station = res.station;
    if (okMsg) toast(okMsg);
    pollStatus();
    return true;
  } catch (e) {
    toast(e.message, true);
    return false;
  }
}
const clone = (o) => JSON.parse(JSON.stringify(o));

// ---------- routing ----------

let route = { name: "monitor", arg: null, query: {} };

function parseRoute() {
  const [path, qs] = (location.hash.replace(/^#\/?/, "") || "monitor").split("?");
  const [name, arg] = path.split("/");
  route = { name: name || "monitor", arg: arg ? decodeURIComponent(arg) : null, query: Object.fromEntries(new URLSearchParams(qs || "")) };
}

function render() {
  parseRoute();
  stopEditorTimers();
  $$(".nav a").forEach((a) => a.classList.toggle("active", a.dataset.nav === route.name && !(route.name === "frequency" && route.arg !== "new")));
  const main = $("#main");
  const views = { monitor: viewMonitor, recordings: viewRecordings, nets: viewNets, net: viewNetEditor, frequency: viewFrequency, settings: viewSettings };
  (views[route.name] || viewMonitor)(main);
  updateChrome();
}

// ---------- chrome: header, banners, disk ----------

function updateChrome() {
  if (!status) return;
  const pill = $("#status-pill"), text = $("#status-text");
  const st = status.state;
  // the pill says what is happening right now: red only while a file is being written
  const writing = status.frequencies.filter((f) => f.recording);
  const n = status.frequencies.length;
  if (st === "armed" && writing.length) {
    pill.className = "pill rec";
    text.textContent = `Recording ${writing.map((f) => fmtMhz(f.mhz)).join(", ")}`;
  } else {
    pill.className = "pill " + ({ armed: "armed", listening: "idle", starting: "warn", waiting: "bad" }[st] || "");
    text.textContent = {
      armed: `Armed · watching ${n} frequenc${n === 1 ? "y" : "ies"}`,
      listening: "Listen only · nothing saved",
      starting: "Starting…",
      waiting: "Receiver not running",
      setup: "Add a frequency to start",
    }[st] || st;
  }
  pill.title = status.started_unix ? `Receiver ${uptime(status.started_unix)}` : "";
  $("#mode-switch").classList.toggle("hidden", st === "setup");
  $$("#mode-switch button").forEach((b) => b.setAttribute("aria-pressed", (b.dataset.mode === "auto") === status.recording_enabled));
  const net = $("#net-pill");
  net.classList.toggle("hidden", !status.active_nets.length);
  net.textContent = status.active_nets.length ? `● ${status.active_nets.join(", ")}` : "";
  $("#window-text").textContent = status.plan ? `Watching ${status.plan.low_mhz.toFixed(2)} – ${status.plan.high_mhz.toFixed(2)} MHz` : "";
  $("#disk-box").innerHTML = `<b>Recordings</b>${bytes(status.disk.used_bytes)} used<br>${status.disk.free_bytes != null ? bytes(status.disk.free_bytes) + " free" : ""}`;
  const banners = $("#banners");
  if (banners) banners.innerHTML = bannerHtml();
}

function bannerHtml() {
  if (!status) return "";
  const out = [];
  if (status.state === "waiting" && status.problem) out.push(`<div class="banner" role="alert">${ICON.alert}<div>${esc(status.problem)}</div></div>`);
  for (const w of status.warnings) out.push(`<div class="banner warn" role="status">${ICON.alert}<div>${esc(w)}</div></div>`);
  return out.join("");
}

async function pollStatus() {
  try {
    status = await api("/api/status");
  } catch {
    $("#status-pill").className = "pill bad";
    $("#status-text").textContent = "App not responding";
    return;
  }
  updateChrome();
  if (route.name === "monitor") updateMonitorLive();
  if (route.name === "nets") updateNetsLive();
}

async function loadRecent(force = false) {
  if (!force && Date.now() - recentLoaded < 30000) return;
  recentLoaded = Date.now();
  try {
    const res = await api(`/api/recordings?from=${Date.now() / 1000 - 86400}`);
    recent = res.recordings;
  } catch { /* keep the old list */ }
}

// ---------- live listening ----------

let audioCtx = null;
let live = null; // { port, timer, gain, since, next }

function stopLive() {
  if (!live) return;
  clearInterval(live.timer);
  live.gain.disconnect();
  live = null;
}

function startLive(port) {
  stopLive();
  if (!audioCtx) audioCtx = new AudioContext();
  audioCtx.resume();
  const gain = audioCtx.createGain();
  gain.connect(audioCtx.destination);
  live = { port, gain, since: 0, next: 0, first: true };
  live.timer = setInterval(async () => {
    const s = live;
    if (!s || s.port !== port) return;
    try {
      const data = await api(`/api/live/${port}/chunk?since=${s.since}`);
      // the first poll skips what was buffered before we started listening
      if (s.first) { s.first = false; s.since = data.next; return; }
      s.since = data.next;
      if (!data.chunk) return;
      const raw = Uint8Array.from(atob(data.chunk), (c) => c.charCodeAt(0));
      const n = Math.floor(raw.length / 4);
      if (!n) return;
      const buf = audioCtx.createBuffer(1, n, status ? status.live_sample_rate : 16000);
      buf.copyToChannel(new Float32Array(raw.buffer, 0, n), 0);
      const src = audioCtx.createBufferSource();
      src.buffer = buf;
      src.connect(s.gain);
      const at = Math.max(audioCtx.currentTime + 0.05, s.next);
      src.start(at);
      s.next = at + buf.duration;
    } catch { /* engine restarting: try again next tick */ }
  }, 250);
}

function previewBeep() {
  if (!audioCtx) audioCtx = new AudioContext();
  audioCtx.resume();
  const t0 = audioCtx.currentTime + 0.1;
  for (const start of [t0, t0 + 0.13]) {
    const osc = audioCtx.createOscillator(), g = audioCtx.createGain();
    osc.frequency.value = 880;
    g.gain.setValueAtTime(0, start);
    g.gain.linearRampToValueAtTime(0.1, start + 0.008);
    g.gain.setValueAtTime(0.1, start + 0.062);
    g.gain.linearRampToValueAtTime(0, start + 0.07);
    osc.connect(g).connect(audioCtx.destination);
    osc.start(start);
    osc.stop(start + 0.08);
  }
}

// ---------- player ----------

const audio = $("#audio");
let playing = null; // recording being played

function playRecording(rec, offset = 0) {
  if (playing && playing.path === rec.path) {
    if (offset) audio.currentTime = offset;
    audio.paused ? audio.play() : offset ? null : audio.pause();
    return;
  }
  playing = rec;
  audio.src = "/audio/" + encodePath(rec.path);
  audio.playbackRate = Number($("#player-speed").value);
  audio.onloadedmetadata = () => { if (offset) audio.currentTime = offset; drawMarks(); };
  audio.play().catch((e) => toast(`Can't play this file: ${e.message}`, true));
  $("#player").classList.remove("hidden");
  const f = station && station.frequencies.find((x) => fmtMhz(x.mhz) === rec.freq);
  $("#player-title").innerHTML = `<b>${esc(rec.freq)}</b>${esc(rec.net || (f ? label(f) : ""))} · ${new Date(rec.start_unix * 1000).toLocaleDateString()} ${clock(rec.start_unix)}`;
  drawMarks();
}

function drawMarks() {
  const total = audio.duration || (playing && playing.duration_sec) || 0;
  $("#player-marks").innerHTML = !playing || !total ? "" : playing.overs.slice(1).map((o) => `<b style="left:${(o.offset_sec / total) * 100}%"></b>`).join("");
}

function syncPlayButtons() {
  $$("[data-play]").forEach((b) => {
    const on = playing && b.dataset.play === playing.path && !audio.paused;
    b.classList.toggle("playing", on);
    b.innerHTML = on ? ICON.pause : ICON.play;
    b.setAttribute("aria-label", on ? "Pause" : "Play");
    const row = b.closest(".rec");
    if (row) row.classList.toggle("current", !!playing && b.dataset.play === playing.path);
  });
  const t = $("#player-toggle");
  t.innerHTML = audio.paused ? ICON.play : ICON.pause;
  t.setAttribute("aria-label", audio.paused ? "Play" : "Pause");
}

audio.addEventListener("play", syncPlayButtons);
audio.addEventListener("pause", syncPlayButtons);
audio.addEventListener("ended", syncPlayButtons);
audio.addEventListener("timeupdate", () => {
  const total = audio.duration || 0;
  $("#player-done").style.width = total ? `${(audio.currentTime / total) * 100}%` : "0";
  $("#player-time").textContent = `${dur(audio.currentTime)} / ${dur(total)}`;
});
$("#player-toggle").addEventListener("click", () => (audio.paused ? audio.play() : audio.pause()));
$("#player-speed").addEventListener("change", (e) => { audio.playbackRate = Number(e.target.value); });
$("#player-close").addEventListener("click", () => { audio.pause(); audio.removeAttribute("src"); playing = null; $("#player").classList.add("hidden"); syncPlayButtons(); });
$("#player-seek").addEventListener("click", (e) => {
  const r = e.currentTarget.getBoundingClientRect();
  if (audio.duration) audio.currentTime = ((e.clientX - r.left) / r.width) * audio.duration;
});
$("#player-seek").addEventListener("keydown", (e) => {
  if (e.key === "ArrowRight") audio.currentTime += 5;
  if (e.key === "ArrowLeft") audio.currentTime -= 5;
});

// ---------- monitor ----------

function viewMonitor(main) {
  if (!station.frequencies.length) {
    main.innerHTML = `<div class="page"><div id="banners"></div><div class="panel empty" style="padding:64px 24px">
      <h3>Add your first frequency</h3>
      <p>Pick any frequency within about 2 MHz of the others, for example your local repeater's output.<br>Recording starts as soon as you add it.</p>
      <p><a class="btn primary big" href="#/frequency/new">Add frequency</a></p></div></div>`;
    return;
  }
  main.innerHTML = `<div class="page">
    <div id="banners">${bannerHtml()}</div>
    <section class="panel" aria-label="Waterfall">
      <div class="panel-head"><h2>Waterfall</h2><span class="muted">Click to add a frequency. Drag a coloured squelch line to set it. Ctrl + wheel zooms.</span>
        <span class="grow"></span><span class="mono" id="wf-readout" style="color:var(--cyan-hi)"></span>
        <label class="gain rw" for="wf-gain">Gain <input type="range" id="wf-gain" min="0" max="49.6" step="0.1" value="${station.receiver.gain_db}"> <b class="mono" id="wf-gain-text">${station.receiver.gain_db.toFixed(1)} dB</b></label></div>
      ${wfToolsHtml()}
      <div class="wf-hint hidden" id="wf-hint" role="status"></div>
      <div class="wf" id="wf" style="height:${wfHeight()}px">
        <canvas id="wf-trace" class="wf-trace" aria-hidden="true"></canvas>
        <canvas id="wf-canvas" class="wf-canvas" role="img" aria-label="Waterfall of the receiver's band"></canvas>
        <div id="wf-markers"></div>
        <div class="wf-cursor hidden" id="wf-cursor"></div>
        <div class="wf-wait" id="wf-wait">Waiting for the receiver…</div>
      </div>
      <div class="wf-resize" id="wf-resize" role="separator" aria-orientation="horizontal" aria-label="Resize the waterfall" tabindex="0"><span></span></div>
      <div class="band-scale" id="band-scale"></div>
    </section>
    <section class="tiles" aria-label="Frequencies">
      ${station.frequencies.map(tileHtml).join("")}
      <a class="add-tile rw" href="#/frequency/new">${ICON.plus}Add frequency<span class="faint">Anything inside the band shown above</span></a>
    </section>
    <section class="panel" aria-label="Latest recordings">
      <div class="panel-head"><h2>Latest recordings</h2><span class="grow"></span><a href="#/recordings">See all</a></div>
      <div id="latest"></div>
    </section>
  </div>`;
  updateMonitorLive();
  setupWaterfall();
  loadRecent(true).then(() => { if (route.name === "monitor") { fillMonitorRecordings(); } });
  fillMonitorRecordings();
}

// ---------- waterfall ----------

const WF_MIN = 140, WF_MAX = 900, WF_DEFAULT = 300, WF_AUTO_RANGE_DB = 45, WF_MIN_RANGE_DB = 6;
const wf = {
  history: [], since: 0, meta: null, floor: null, timer: null, lut: null, channels: [],
  peak: null, suppressClick: false, drag: null,
  // per-viewer display settings, kept in this browser
  opts: { auto: true, floor: -95, ceil: -50, contrast: 1, peak: false, lo: 0, hi: 1 },
};

function wfLoadOpts() {
  try { Object.assign(wf.opts, JSON.parse(localStorage.getItem("wfOpts") || "{}")); } catch { /* defaults */ }
}
function wfSaveOpts() {
  try { localStorage.setItem("wfOpts", JSON.stringify(wf.opts)); } catch { /* private window */ }
}
wfLoadOpts();

function wfHeight() {
  try { return Math.max(WF_MIN, Math.min(WF_MAX, Number(localStorage.getItem("wfHeight")) || WF_DEFAULT)); } catch { return WF_DEFAULT; }
}

/** The colour scale in dB: automatic from the noise floor, or the user's floor and ceiling. */
function wfLevels() {
  if (wf.opts.auto) {
    const floor = wf.floor ?? -100;
    return { floor, ceil: floor + WF_AUTO_RANGE_DB };
  }
  return { floor: wf.opts.floor, ceil: Math.max(wf.opts.ceil, wf.opts.floor + WF_MIN_RANGE_DB) };
}

/** The bins in view: [first, end). */
function wfBins() {
  const bins = wf.meta.bins;
  let b0 = Math.floor(wf.opts.lo * bins), b1 = Math.ceil(wf.opts.hi * bins);
  if (b1 - b0 < 8) { b1 = Math.min(bins, b0 + 8); b0 = b1 - 8; }
  return [Math.max(0, b0), Math.min(bins, b1)];
}

function wfLut() {
  if (wf.lut) return wf.lut;
  const stops = [[0, 8, 12, 20], [0.35, 12, 46, 74], [0.6, 18, 150, 165], [0.82, 255, 176, 32], [1, 255, 246, 225]];
  const lut = new Uint8ClampedArray(256 * 3);
  for (let i = 0; i < 256; i++) {
    const v = i / 255;
    let k = 1;
    while (k < stops.length - 1 && v > stops[k][0]) k++;
    const a = stops[k - 1], b = stops[k], t = (v - a[0]) / (b[0] - a[0]);
    for (let c = 0; c < 3; c++) lut[i * 3 + c] = a[c + 1] + (b[c + 1] - a[c + 1]) * Math.max(0, Math.min(1, t));
  }
  return (wf.lut = lut);
}

function wfRow(line, img, row, b0, lv) {
  const lut = wfLut(), span = lv.ceil - lv.floor, gamma = wf.opts.contrast;
  const off = row * img.width * 4;
  for (let x = 0; x < img.width; x++) {
    let v = (line[b0 + x] - lv.floor) / span;
    v = v <= 0 ? 0 : v >= 1 ? 1 : gamma === 1 ? v : Math.pow(v, gamma);
    const i = Math.round(v * 255) * 3;
    img.data[off + x * 4] = lut[i];
    img.data[off + x * 4 + 1] = lut[i + 1];
    img.data[off + x * 4 + 2] = lut[i + 2];
    img.data[off + x * 4 + 3] = 255;
  }
}

function updateFloor(line) {
  const sorted = Float32Array.from(line).sort();
  const noise = sorted[Math.floor(sorted.length * 0.3)] - 3;
  wf.floor = wf.floor == null ? noise : wf.floor * 0.95 + noise * 0.05;
}

function updatePeak(line) {
  if (!wf.peak || wf.peak.length !== line.length) wf.peak = Float32Array.from(line);
  const decay = 6 / ((station && station.receiver.spectrum_rate) || 15); // about 6 dB a second
  for (let i = 0; i < line.length; i++) wf.peak[i] = Math.max(line[i], wf.peak[i] - decay);
}

function wfRedraw() {
  const canvas = $("#wf-canvas");
  if (!canvas || !wf.meta) return;
  const [b0, b1] = wfBins(), lv = wfLevels();
  const h = canvas.clientHeight || 1;
  canvas.width = b1 - b0;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  const img = ctx.createImageData(canvas.width, h);
  for (let r = 0; r < h && r < wf.history.length; r++) wfRow(wf.history[wf.history.length - 1 - r], img, r, b0, lv);
  ctx.putImageData(img, 0, 0);
  wfTrace();
  wfMarkers();
}

function wfAppend(lines) {
  const canvas = $("#wf-canvas");
  if (!canvas || !lines.length) return;
  const [b0, b1] = wfBins();
  if (canvas.width !== b1 - b0 || canvas.height !== canvas.clientHeight) return wfRedraw();
  const ctx = canvas.getContext("2d"), lv = wfLevels();
  const n = Math.min(lines.length, canvas.height);
  ctx.drawImage(canvas, 0, 0, canvas.width, canvas.height - n, 0, n, canvas.width, canvas.height - n);
  const img = ctx.createImageData(canvas.width, n);
  for (let r = 0; r < n; r++) wfRow(lines[lines.length - 1 - r], img, r, b0, lv);
  ctx.putImageData(img, 0, 0);
  wfTrace();
}

/** Trace geometry shared by drawing and squelch-line dragging. */
function wfTraceScale(h) {
  const lv = wfLevels(), lo = lv.floor - 3, span = lv.ceil - lv.floor + 6;
  return {
    y: (v) => h - Math.max(0, Math.min(1, (v - lo) / span)) * (h - 4) - 2,
    db: (y) => lo + ((h - 2 - y) / (h - 4)) * span,
  };
}

function wfSquelchLines(w, h) {
  const sc = wfTraceScale(h);
  const out = [];
  for (const f of station.frequencies) {
    const ch = (wf.channels || []).find((c) => Math.abs(c.freq_hz - f.mhz * 1e6) < 1000);
    const pct = wfPct(f.mhz);
    if (!ch || pct < 0 || pct > 100) continue;
    out.push({ f, ch, x: (pct / 100) * w, y: sc.y(ch.noise_db + f.squelch * 2) });
  }
  return out;
}

function wfTrace() {
  const c = $("#wf-trace");
  const line = wf.history[wf.history.length - 1];
  if (!c || !line || !wf.meta) return;
  const w = c.clientWidth, h = c.clientHeight, dpr = window.devicePixelRatio || 1;
  c.width = w * dpr; c.height = h * dpr;
  const ctx = c.getContext("2d");
  ctx.scale(dpr, dpr);
  const { y } = wfTraceScale(h);
  const [b0, b1] = wfBins(), n = b1 - b0;
  const path = (data) => {
    ctx.beginPath();
    for (let i = 0; i < n; i++) {
      const x = n > 1 ? (i / (n - 1)) * w : 0;
      i ? ctx.lineTo(x, y(data[b0 + i])) : ctx.moveTo(x, y(data[b0 + i]));
    }
  };
  if (wf.opts.peak && wf.peak) {
    path(wf.peak);
    ctx.strokeStyle = "rgba(255,176,32,0.55)";
    ctx.lineWidth = 1;
    ctx.stroke();
  }
  path(line);
  ctx.strokeStyle = "#8be8fa";
  ctx.lineWidth = 1.2;
  ctx.stroke();
  ctx.lineTo(w, h); ctx.lineTo(0, h); ctx.closePath();
  ctx.fillStyle = "rgba(61,214,245,0.12)";
  ctx.fill();

  // each frequency's squelch: the engine's noise floor for it plus 2 dB per slider step; drag it to change
  ctx.font = "11px Cascadia Mono, Consolas, monospace";
  for (const s of wfSquelchLines(w, h)) {
    const colour = freqColour(s.f);
    const active = wf.drag && wf.drag.id === s.f.id;
    ctx.strokeStyle = colour;
    ctx.lineWidth = active ? 3 : 2;
    ctx.beginPath(); ctx.moveTo(s.x - 26, s.y); ctx.lineTo(s.x + 26, s.y); ctx.stroke();
    ctx.fillStyle = colour;
    ctx.fillText(`SQ ${s.f.squelch}`, s.x + 30, Math.max(10, s.y + 4));
    ctx.beginPath();
    ctx.arc(s.x, y(s.ch.signal_db), 3.5, 0, Math.PI * 2);
    ctx.fillStyle = s.ch.open ? "#ffb020" : colour;
    ctx.fill();
  }
  ctx.fillStyle = "#8b9aab";
  ctx.fillText(`Gain ${station.receiver.gain_db.toFixed(1)} dB`, 6, 13);
}

const wfLowHz = () => wf.meta.centre_hz - wf.meta.sample_rate / 2;
const wfPct = (mhz) => (((mhz * 1e6 - wfLowHz()) / wf.meta.sample_rate - wf.opts.lo) / (wf.opts.hi - wf.opts.lo)) * 100;

function wfMarkers() {
  const box = $("#wf-markers");
  if (!box || !wf.meta) return;
  box.innerHTML = station.frequencies.map((f) => {
    const pct = wfPct(f.mhz);
    return pct < 0 || pct > 100 ? "" : `<div class="marker" data-marker="${esc(f.id)}" style="left:${pct}%;--fc:${freqColour(f)}"><div class="flag">${fmtMhz(f.mhz)}${f.name ? " " + esc(f.name) : ""}</div></div>`;
  }).join("");
  const sr = wf.meta.sample_rate, lo = (wfLowHz() + wf.opts.lo * sr) / 1e6, span = ((wf.opts.hi - wf.opts.lo) * sr) / 1e6;
  const digits = span < 0.3 ? 3 : 2;
  $("#band-scale").innerHTML = Array.from({ length: 6 }, (_, i) => `<span>${(lo + (i * span) / 5).toFixed(digits)}</span>`).join("");
  updateMonitorLive();
}

function wfFreqAt(clientX) {
  const r = $("#wf").getBoundingClientRect();
  const frac = wf.opts.lo + ((clientX - r.left) / r.width) * (wf.opts.hi - wf.opts.lo);
  return (wfLowHz() + frac * wf.meta.sample_rate) / 1e6;
}

/** Tips for the gain, from the latest lines. */
function wfGainHint() {
  const el = $("#wf-hint");
  if (!el) return;
  const recent = wf.history.slice(-15);
  if (!recent.length || wf.floor == null) { el.classList.add("hidden"); return; }
  let peak = -200;
  for (const l of recent) for (let i = 0; i < l.length; i++) if (l[i] > peak) peak = l[i];
  const noise = wf.floor + 3, gain = station.receiver.gain_db;
  let msg = "";
  if (peak > -8) msg = `A very strong signal (${peak.toFixed(0)} dB) is close to overloading the dongle. Lower the gain a few dB to keep it clean.`;
  else if (noise > -55) msg = `The noise floor is high (${noise.toFixed(0)} dB). Lowering the gain usually makes signals stand out more.`;
  else if (gain < 12 && noise < -100) msg = "The gain is very low: weak stations may not open the squelch. Try 25–35 dB.";
  el.textContent = msg;
  el.classList.toggle("hidden", !msg);
}

async function wfPoll() {
  if (route.name !== "monitor" || !$("#wf")) return;
  try {
    const d = await api(`/api/spectrum?since=${wf.since}`);
    $("#wf-wait").classList.toggle("hidden", d.live || wf.history.length > 0);
    if (!d.count) return;
    const raw = Uint8Array.from(atob(d.data), (c) => c.charCodeAt(0));
    const all = new Float32Array(raw.buffer);
    const meta = { bins: d.bins, centre_hz: d.centre_hz, sample_rate: d.sample_rate };
    const retuned = !wf.meta || wf.meta.bins !== meta.bins || wf.meta.centre_hz !== meta.centre_hz;
    if (retuned) { wf.history = []; wf.peak = null; }
    wf.meta = meta;
    wf.since = d.next;
    wf.channels = d.channels;
    const lines = [];
    for (let i = 0; i < d.count; i++) lines.push(all.subarray(i * d.bins, (i + 1) * d.bins));
    lines.forEach((l) => { updateFloor(l); updatePeak(l); });
    wf.history.push(...lines);
    if (wf.history.length > WF_MAX) wf.history.splice(0, wf.history.length - WF_MAX);
    if (wf.opts.auto) wfShowLevels();
    retuned ? wfRedraw() : wfAppend(lines);
    wfGainHint();
  } catch { /* engine restarting */ }
}

// ----- toolbar -----

function wfToolsHtml() {
  const o = wf.opts, rate = station.receiver.spectrum_rate || 15;
  return `<div class="wf-tools" id="wf-tools">
    <div class="grp" role="group" aria-label="Colour levels">
      <button type="button" class="chip" id="wf-auto" aria-pressed="${o.auto}" title="Set the levels from the noise floor automatically">Auto levels</button>
      <label for="wf-floor">Floor</label><input type="range" id="wf-floor" min="-130" max="-10" step="1" value="${o.floor}"><b id="wf-floor-text"></b>
      <label for="wf-ceil">Ceiling</label><input type="range" id="wf-ceil" min="-120" max="10" step="1" value="${o.ceil}"><b id="wf-ceil-text"></b>
    </div>
    <div class="grp"><label for="wf-contrast" title="Higher pushes the noise darker so transmissions stand out">Contrast</label>
      <input type="range" id="wf-contrast" min="0.5" max="3" step="0.1" value="${o.contrast}"><b id="wf-contrast-text">${Number(o.contrast).toFixed(1)}×</b></div>
    <div class="grp"><button type="button" class="chip" id="wf-peak" aria-pressed="${o.peak}" title="A faint line that remembers each spot's highest level for a few seconds">Peak hold</button></div>
    <div class="grp" role="group" aria-label="Zoom">
      <span>Zoom</span>
      <button type="button" class="btn" data-zoom="out" aria-label="Zoom out">−</button>
      <button type="button" class="btn" data-zoom="in" aria-label="Zoom in">+</button>
      <button type="button" class="btn" data-zoom="fit" title="Frame your frequencies">Fit</button>
      <button type="button" class="btn" data-zoom="full" title="The whole band">Full</button>
    </div>
    <div class="grp rw"><label for="wf-speed" title="Changing it restarts the receiver for a moment">Speed</label>
      <select id="wf-speed">${[5, 10, 15, 20, 30].map((v) => `<option value="${v}" ${v === rate ? "selected" : ""}>${v} lines/s</option>`).join("")}</select></div>
  </div>`;
}

function wfShowLevels() {
  const lv = wfLevels();
  const fl = $("#wf-floor"), ce = $("#wf-ceil");
  if (!fl) return;
  if (wf.opts.auto) { fl.value = Math.round(lv.floor); ce.value = Math.round(lv.ceil); }
  $("#wf-floor-text").textContent = `${Math.round(lv.floor)} dB`;
  $("#wf-ceil-text").textContent = `${Math.round(lv.ceil)} dB`;
  $("#wf-auto").setAttribute("aria-pressed", wf.opts.auto);
}

function wfSetView(lo, hi) {
  const span = Math.max(0.02, Math.min(1, hi - lo));
  lo = Math.max(0, Math.min(1 - span, lo));
  wf.opts.lo = lo;
  wf.opts.hi = lo + span;
  wfSaveOpts();
  wfRedraw();
}

function wfZoom(kind, aroundFrac = null) {
  const { lo, hi } = wf.opts, span = hi - lo, mid = aroundFrac ?? (lo + hi) / 2;
  if (kind === "full") return wfSetView(0, 1);
  if (kind === "fit") {
    if (!wf.meta || !station.frequencies.length) return wfSetView(0, 1);
    const sr = wf.meta.sample_rate, low = wfLowHz();
    const fr = station.frequencies.map((f) => (f.mhz * 1e6 - low) / sr);
    const margin = 150e3 / sr;
    return wfSetView(Math.min(...fr) - margin, Math.max(...fr) + margin);
  }
  const k = kind === "in" ? 0.5 : 2;
  const nl = mid - (mid - lo) * k;
  wfSetView(nl, nl + span * k);
}

function setupWaterfall() {
  clearInterval(wf.timer);
  wf.timer = setInterval(wfPoll, 150);
  wfShowLevels();
  if (wf.meta) wfRedraw();
  wfPoll();
  const box = $("#wf"), cursor = $("#wf-cursor"), readout = $("#wf-readout"), trace = $("#wf-trace");

  // levels
  const manual = () => { wf.opts.auto = false; };
  $("#wf-auto").addEventListener("click", () => {
    wf.opts.auto = !wf.opts.auto;
    if (!wf.opts.auto) { const lv = wfLevels(); wf.opts.floor = Math.round(lv.floor); wf.opts.ceil = Math.round(lv.ceil); }
    wfSaveOpts(); wfShowLevels(); wfRedraw();
  });
  $("#wf-floor").addEventListener("input", (e) => {
    if (wf.opts.auto) { const lv = wfLevels(); wf.opts.ceil = Math.round(lv.ceil); }
    manual();
    wf.opts.floor = Math.min(Number(e.target.value), wf.opts.ceil - WF_MIN_RANGE_DB);
    wfSaveOpts(); wfShowLevels(); wfRedraw();
  });
  $("#wf-ceil").addEventListener("input", (e) => {
    if (wf.opts.auto) { const lv = wfLevels(); wf.opts.floor = Math.round(lv.floor); }
    manual();
    wf.opts.ceil = Math.max(Number(e.target.value), wf.opts.floor + WF_MIN_RANGE_DB);
    wfSaveOpts(); wfShowLevels(); wfRedraw();
  });
  $("#wf-contrast").addEventListener("input", (e) => {
    wf.opts.contrast = Number(e.target.value);
    $("#wf-contrast-text").textContent = `${wf.opts.contrast.toFixed(1)}×`;
    wfSaveOpts(); wfRedraw();
  });
  $("#wf-peak").addEventListener("click", (e) => {
    wf.opts.peak = !wf.opts.peak;
    e.currentTarget.setAttribute("aria-pressed", wf.opts.peak);
    wfSaveOpts(); wfTrace();
  });
  $$("[data-zoom]").forEach((b) => b.addEventListener("click", () => wfZoom(b.dataset.zoom)));
  $("#wf-speed").addEventListener("change", (e) => {
    const next = clone(station);
    next.receiver.spectrum_rate = Number(e.target.value);
    saveStation(next, `Waterfall at ${e.target.value} lines a second. The receiver restarts for a moment.`);
  });

  // a gain change restarts the receiver (about 2 s), so it is saved when the slider is let go
  $("#wf-gain").addEventListener("input", (e) => { $("#wf-gain-text").textContent = `${Number(e.target.value).toFixed(1)} dB`; });
  $("#wf-gain").addEventListener("change", (e) => {
    const g = Number(e.target.value);
    station.receiver.gain_db = g;
    saveStation(clone(station), `Gain ${g.toFixed(1)} dB. The receiver restarts for a moment.`);
  });
  $(".gain").addEventListener("click", (e) => e.stopPropagation());

  // cursor readout
  box.addEventListener("mousemove", (e) => {
    if (!wf.meta) return;
    const r = box.getBoundingClientRect();
    cursor.classList.remove("hidden");
    cursor.style.left = `${e.clientX - r.left}px`;
    readout.textContent = `${wfFreqAt(e.clientX).toFixed(4)} MHz`;
  });
  box.addEventListener("mouseleave", () => { cursor.classList.add("hidden"); readout.textContent = ""; });

  // Ctrl + wheel zooms around the cursor
  box.addEventListener("wheel", (e) => {
    if (!e.ctrlKey || !wf.meta) return;
    e.preventDefault();
    const r = box.getBoundingClientRect();
    wfZoom(e.deltaY < 0 ? "in" : "out", wf.opts.lo + ((e.clientX - r.left) / r.width) * (wf.opts.hi - wf.opts.lo));
  }, { passive: false });

  // drag a squelch line on the trace; drag the waterfall sideways to pan when zoomed
  const hitLine = (e) => {
    const r = trace.getBoundingClientRect();
    const x = e.clientX - r.left, y = e.clientY - r.top;
    return wfSquelchLines(r.width, r.height).find((s) => Math.abs(x - s.x) < 34 && Math.abs(y - s.y) < 8);
  };
  trace.addEventListener("mousemove", (e) => { if (!wf.drag) trace.classList.toggle("grab", !!(wf.meta && hitLine(e))); });
  box.addEventListener("pointerdown", (e) => {
    if (!wf.meta || e.button !== 0) return;
    const hit = e.target === trace && !client.read_only ? hitLine(e) : null;
    wf.suppressClick = false;
    if (hit) {
      wf.drag = { kind: "squelch", id: hit.f.id, noise: hit.ch.noise_db };
      trace.classList.add("dragging");
    } else {
      wf.drag = { kind: "pan", x: e.clientX, lo: wf.opts.lo, hi: wf.opts.hi, moved: false };
    }
    box.setPointerCapture(e.pointerId);
  });
  box.addEventListener("pointermove", (e) => {
    const d = wf.drag;
    if (!d) return;
    if (d.kind === "squelch") {
      const r = trace.getBoundingClientRect();
      const db = wfTraceScale(r.height).db(e.clientY - r.top);
      const sq = Math.max(0, Math.min(9, Math.round((db - d.noise) / 2)));
      const f = station.frequencies.find((x) => x.id === d.id);
      if (f && f.squelch !== sq) setTileSquelch(f, sq, false);
      wf.suppressClick = true;
      return;
    }
    const dx = e.clientX - d.x;
    if (Math.abs(dx) > 4) d.moved = true;
    if (d.moved && d.hi - d.lo < 1) {
      box.classList.add("panning");
      const shift = (-dx / box.clientWidth) * (d.hi - d.lo);
      wf.opts.lo = Math.max(0, Math.min(1 - (d.hi - d.lo), d.lo + shift));
      wf.opts.hi = wf.opts.lo + (d.hi - d.lo);
      wfRedraw();
    }
  });
  const endDrag = () => {
    const d = wf.drag;
    if (!d) return;
    wf.drag = null;
    trace.classList.remove("dragging");
    box.classList.remove("panning");
    if (d.kind === "squelch") {
      const f = station.frequencies.find((x) => x.id === d.id);
      if (f) setTileSquelch(f, f.squelch, true);
      wfTrace();
    } else if (d.moved) {
      wf.suppressClick = true;
      wfSaveOpts();
    }
  };
  box.addEventListener("pointerup", endDrag);
  box.addEventListener("pointercancel", endDrag);

  box.addEventListener("click", (e) => {
    if (!wf.meta || wf.suppressClick || client.read_only) { wf.suppressClick = false; return; }
    const mhz = wfFreqAt(e.clientX);
    const near = station.frequencies.find((f) => Math.abs(f.mhz - mhz) < 0.01);
    if (near) { location.hash = `#/frequency/${encodeURIComponent(near.id)}`; return; }
    location.hash = `#/frequency/new?mhz=${(Math.round(mhz / 0.0125) * 0.0125).toFixed(4)}`;
  });

  // resize by the bottom edge
  const handle = $("#wf-resize");
  const setHeight = (h, save) => {
    h = Math.max(WF_MIN, Math.min(WF_MAX, Math.round(h)));
    box.style.height = `${h}px`;
    wfRedraw();
    if (save) try { localStorage.setItem("wfHeight", String(h)); } catch { /* private window */ }
  };
  handle.addEventListener("pointerdown", (e) => {
    const startY = e.clientY, startH = box.clientHeight;
    handle.setPointerCapture(e.pointerId);
    const move = (ev) => setHeight(startH + ev.clientY - startY, false);
    const up = (ev) => { handle.removeEventListener("pointermove", move); handle.removeEventListener("pointerup", up); setHeight(startH + ev.clientY - startY, true); };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
  });
  handle.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") { setHeight(box.clientHeight + 20, true); e.preventDefault(); }
    if (e.key === "ArrowUp") { setHeight(box.clientHeight - 20, true); e.preventDefault(); }
  });
}

window.addEventListener("resize", () => { if (route.name === "monitor") wfTrace(); });

function tileHtml(f) {
  const mode = f.modulation === "am" ? "AM" : "FM";
  return `<article class="tile" data-tile="${esc(f.id)}" style="--fc:${freqColour(f)}">
    <div class="tile-head">
      <span class="dot"></span><span class="swatch" aria-hidden="true"></span><span class="name">${esc(label(f))}</span><span class="tag">${mode}</span>
      ${f.ctcss_hz ? `<span class="tag">CTCSS ${f.ctcss_hz}</span>` : ""}${f.record ? "" : '<span class="tag">Not recorded</span>'}
      <span class="tag amber hidden" data-netname></span>
      <span class="grow"></span><span class="state" data-state>IDLE</span>
      <a class="btn ghost icon rw" href="#/frequency/${encodeURIComponent(f.id)}" aria-label="Edit ${esc(label(f))}">${ICON.edit}</a>
    </div>
    <div><span class="freq">${fmtMhz(f.mhz)}</span><span class="unit">MHz</span></div>
    <div class="meter" aria-hidden="true"><i data-meter></i></div>
    <div class="squelch">
      <label for="sq-${esc(f.id)}">Squelch</label>
      <input type="range" id="sq-${esc(f.id)}" min="0" max="9" step="1" value="${f.squelch}" data-squelch-id="${esc(f.id)}">
      <b class="mono" data-squelch="${esc(f.id)}">${f.squelch}</b>
    </div>
    <div class="hint edge ${f.squelch === 0 && f.record ? "" : "hidden"}" data-open-warning="${esc(f.id)}">Squelch 0 is always open${f.record ? ": this frequency records constantly" : ""}.</div>
    <div class="strip" data-strip aria-label="Activity in the last 24 hours"></div>
    <div class="strip-scale"><span>24 h ago</span><span>now</span></div>
    <div class="tile-foot">
      <button class="btn" type="button" data-act="listen" data-id="${esc(f.id)}">${ICON.headphones}<span>Listen</span></button>
      <a href="#/recordings?freq=${encodeURIComponent(fmtMhz(f.mhz))}" data-clips></a>
      <span class="grow"></span><span class="muted" data-heard style="font-size:12px"></span>
    </div>
  </article>`;
}

function updateMonitorLive() {
  if (!status || !station) return;
  for (const fs of status.frequencies) {
    const tile = $(`[data-tile="${CSS.escape(fs.id)}"]`);
    if (!tile) continue;
    tile.classList.toggle("on", fs.on_air);
    tile.classList.toggle("rec", fs.recording);
    const stateEl = $("[data-state]", tile);
    stateEl.textContent = fs.recording ? "REC" : fs.on_air ? "ON AIR" : "IDLE";
    stateEl.classList.toggle("rec", fs.recording);
    stateEl.title = fs.recording ? "Being recorded now" : fs.on_air ? "Heard, but not saved" : "";
    const level = fs.on_air && fs.level_dbfs != null ? Math.max(4, Math.min(100, ((fs.level_dbfs + 60) / 60) * 100)) : 0;
    $("[data-meter]", tile).style.width = `${level}%`;
    $("[data-heard]", tile).textContent = fs.recording ? (fs.net ? `Recording ${fs.net}` : "Recording now") : fs.on_air ? "On air · not saved" : ago(fs.last_heard_unix);
    const netTag = $("[data-netname]", tile);
    netTag.classList.toggle("hidden", !fs.net);
    netTag.textContent = fs.net ? fs.net : "";
    const marker = $(`[data-marker="${CSS.escape(fs.id)}"]`);
    if (marker) marker.classList.toggle("on", fs.on_air);
    const btn = $(`[data-act="listen"][data-id="${CSS.escape(fs.id)}"]`);
    if (btn) {
      const on = live && live.port === fs.port;
      btn.classList.toggle("on", !!on);
      btn.setAttribute("aria-pressed", on ? "true" : "false");
      $("span", btn).textContent = on ? (fs.on_air ? "Listening" : "Listening · quiet") : "Listen";
    }
  }
}

function fillMonitorRecordings() {
  const now = Date.now() / 1000, today = unixOf(startOfDay(new Date()));
  for (const f of station.frequencies) {
    const tile = $(`[data-tile="${CSS.escape(f.id)}"]`);
    if (!tile) continue;
    const mine = recent.filter((r) => r.freq === fmtMhz(f.mhz));
    const marks = mine.flatMap((r) => (r.overs.length ? r.overs.map((o) => [o.start_unix, o.duration_sec]) : [[r.start_unix, r.duration_sec || 1]]));
    $("[data-strip]", tile).innerHTML = marks
      .filter(([s]) => s > now - 86400)
      .map(([s, d]) => `<i style="left:${((s - (now - 86400)) / 86400) * 100}%;width:${Math.max(0.15, (d / 86400) * 100)}%"></i>`).join("");
    const n = mine.filter((r) => r.start_unix >= today).length;
    $("[data-clips]", tile).textContent = n === 1 ? "1 recording today" : `${n} recordings today`;
  }
  const latest = $("#latest");
  if (latest) latest.innerHTML = recent.length ? recent.slice(0, 6).map(recRowHtml).join("") : `<div class="empty">Nothing recorded yet. Recordings appear here as soon as someone transmits.</div>`;
  syncPlayButtons();
}

function oversBar(rec) {
  const total = rec.duration_sec || 1;
  return rec.overs.map((o) => `<i style="left:${(o.offset_sec / total) * 100}%;width:${(o.duration_sec / total) * 100}%"></i>`).join("");
}

function recRowHtml(rec, opts = {}) {
  const f = station.frequencies.find((x) => fmtMhz(x.mhz) === rec.freq);
  const who = rec.net || (f ? f.name : "") || "";
  const n = rec.overs.length;
  return `<div class="rec">
    <button class="btn round" type="button" data-play="${esc(rec.path)}" aria-label="Play">${ICON.play}</button>
    <span class="f">${esc(rec.freq)}</span>
    <span class="t">${clock(rec.start_unix)}</span>
    <span class="n">${esc(who)}${n > 1 ? ` · ${n} overs` : ""}</span>
    <div class="overs">${oversBar(rec)}</div>
    <span class="d">${rec.duration_sec != null ? dur(rec.duration_sec) : bytes(rec.bytes)}</span>
    ${opts.actions ? `<button class="btn ghost icon" type="button" data-act="reveal" data-path="${esc(rec.path)}" aria-label="Show in folder">${ICON.folder}</button>
      <a class="btn ghost icon dl" href="/audio/${encodePath(rec.path)}?download=1" download aria-label="Download">${ICON.download}</a>
      <button class="btn ghost icon rw" type="button" data-act="delete-rec" data-path="${esc(rec.path)}" aria-label="Delete">${ICON.trash}</button>` : ""}
  </div>`;
}

// ---------- recordings ----------

let recDay = startOfDay(new Date());
let recList = [];

function viewRecordings(main) {
  const freqFilter = route.query.freq || "all";
  main.innerHTML = `<div class="page">
    <div id="banners">${bannerHtml()}</div>
    <div class="page-head">
      <h1>Recordings</h1><span class="grow"></span>
      <button class="btn icon" type="button" data-act="day" data-delta="-1" aria-label="Previous day">${ICON.left}</button>
      <input type="date" id="rec-date" value="${dateInput(recDay)}" aria-label="Day">
      <button class="btn icon" type="button" data-act="day" data-delta="1" aria-label="Next day">${ICON.right}</button>
      <button class="btn" type="button" data-act="day" data-delta="0">Today</button>
      <button class="btn" type="button" data-act="reveal">${ICON.folder}Open folder</button>
    </div>
    <section class="panel" aria-label="Day timeline">
      <div class="panel-head"><h2 id="day-title"></h2><span class="muted">Click a mark to hear it</span></div>
      <div class="lanes" id="lanes"></div>
      <div class="summary" id="day-summary" aria-label="Day summary"></div>
      <div class="hours"><span>00</span><span>04</span><span>08</span><span>12</span><span>16</span><span>20</span><span>24</span></div>
      <div style="height:12px"></div>
    </section>
    <div class="chips" role="group" aria-label="Show" id="rec-filters" data-filter="${esc(freqFilter)}"></div>
    <div id="rec-nets" style="display:flex;flex-direction:column;gap:12px"></div>
    <section class="panel" aria-label="Clips" id="rec-clips"></section>
  </div>`;
  $("#rec-date").addEventListener("change", (e) => { if (e.target.value) { recDay = startOfDay(new Date(e.target.value + "T00:00")); loadDay(); } });
  loadDay();
}

async function loadDay() {
  const from = unixOf(recDay), to = from + 86400;
  $("#day-title").textContent = recDay.toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" });
  $("#rec-date").value = dateInput(recDay);
  try {
    recList = (await api(`/api/recordings?from=${from}&to=${to}`)).recordings;
  } catch (e) {
    toast(e.message, true);
    recList = [];
  }
  drawDay();
}

function drawDay() {
  const from = unixOf(recDay);
  const freqs = [...new Set([...station.frequencies.map((f) => fmtMhz(f.mhz)), ...recList.map((r) => r.freq).filter(Boolean)])];
  $("#lanes").innerHTML = freqs.map((fq) => {
    const recs = recList.filter((r) => r.freq === fq);
    const f = station.frequencies.find((x) => fmtMhz(x.mhz) === fq);
    const marks = recs.flatMap((r) => (r.overs.length ? r.overs : [{ start_unix: r.start_unix, duration_sec: r.duration_sec || 1, offset_sec: 0 }]).map((o) =>
      `<i title="${clockSec(o.start_unix)}" data-act="play-at" data-path="${esc(r.path)}" data-offset="${o.offset_sec}" style="left:${((o.start_unix - from) / 86400) * 100}%;width:${Math.max(0.12, (o.duration_sec / 86400) * 100)}%"></i>`)).join("");
    return `<div class="lane"><span class="who" title="${esc(f ? label(f) : fq)}">${esc(fq)}</span><div class="track">${marks}</div><span class="count">${recs.length} rec.</span></div>`;
  }).join("") || `<div class="muted">No frequencies yet.</div>`;

  $("#day-summary").innerHTML = daySummaryHtml(recList);

  const filter = $("#rec-filters").dataset.filter;
  const opts = [["all", "All"], ...freqs.map((f) => [f, f]), ["nets", "Nets"]];
  $("#rec-filters").innerHTML = opts.map(([v, t]) => `<button type="button" class="chip" data-act="rec-filter" data-value="${esc(v)}" aria-pressed="${v === filter}">${esc(t)}</button>`).join("");
  const shown = recList.filter((r) => filter === "all" || (filter === "nets" ? r.net : r.freq === filter));
  const nets = shown.filter((r) => r.net);
  const clips = shown.filter((r) => !r.net);
  $("#rec-nets").innerHTML = nets.map((r) => `<article class="net-card">
      <div style="display:flex;align-items:center;gap:12px;flex-wrap:wrap">
        <button class="btn round" type="button" data-play="${esc(r.path)}" aria-label="Play">${ICON.play}</button>
        <div><div style="font-weight:600">${esc(r.net)}${r.freq ? ` · <span class="mono">${esc(r.freq)}</span>` : ""}</div>
        <div class="muted" style="font-size:12px">${clock(r.start_unix)} · ${r.overs.length ? `${dur(r.duration_sec)} of speech, silences removed · ${r.overs.length} overs` : bytes(r.bytes)}</div></div>
        <span class="grow"></span>
        <button class="btn" type="button" data-act="reveal" data-path="${esc(r.path)}">${ICON.folder}Show in folder</button>
        <a class="btn dl" href="/audio/${encodePath(r.path)}?download=1" download>${ICON.download}Download</a>
        <button class="btn ghost icon rw" type="button" data-act="delete-rec" data-path="${esc(r.path)}" aria-label="Delete">${ICON.trash}</button>
      </div>
      <div class="overs">${oversBar(r)}${r.overs.slice(1).map((o) => `<b style="left:${(o.offset_sec / (r.duration_sec || 1)) * 100}%"></b>`).join("")}</div>
    </article>`).join("");
  $("#rec-clips").innerHTML = clips.length
    ? clips.map((r) => recRowHtml(r, { actions: true })).join("")
    : `<div class="empty">${recList.length ? "Nothing here with this filter." : "Nothing was recorded on this day."}</div>`;
  if (!nets.length && filter === "nets") $("#rec-clips").innerHTML = `<div class="empty">No net was recorded on this day.</div>`;
  syncPlayButtons();
}

/** The day at a glance: overs and time on air, per frequency and per net. */
function daySummaryHtml(recs) {
  if (!recs.length) return "";
  const oversOf = (r) => (r.overs.length ? r.overs : [{ start_unix: r.start_unix, duration_sec: r.duration_sec || 0 }]);
  const tally = (list) => list.reduce((t, r) => { for (const o of oversOf(r)) { t.n++; t.sec += o.duration_sec || 0; } return t; }, { n: 0, sec: 0 });
  const mins = (sec) => (sec < 60 ? `${Math.round(sec)} s` : `${Math.round(sec / 60)} min`);
  const all = tally(recs);
  const out = [`<div class="stat"><b>${all.n}</b>over${all.n === 1 ? "" : "s"} · ${mins(all.sec)} on air</div>`];
  const freqs = [...new Set(recs.map((r) => r.freq).filter(Boolean))];
  for (const fq of freqs) {
    const t = tally(recs.filter((r) => r.freq === fq));
    const f = station.frequencies.find((x) => fmtMhz(x.mhz) === fq);
    out.push(`<div class="stat f" style="--fc:${f ? freqColour(f) : "var(--amber)"}"><b>${esc(fq)}</b>${t.n} over${t.n === 1 ? "" : "s"} · ${mins(t.sec)}</div>`);
  }
  for (const r of recs.filter((x) => x.net)) {
    const ov = oversOf(r), last = ov[ov.length - 1];
    const end = last.start_unix + (last.duration_sec || 0);
    out.push(`<div class="stat"><b>${esc(r.net)}</b>${clock(r.start_unix)}–${clock(end)} · ${ov.length} over${ov.length === 1 ? "" : "s"}</div>`);
  }
  return out.join("");
}

// ---------- nets ----------

function nextStart(net, now = new Date()) {
  const [h, m] = net.start.split(":").map(Number);
  for (let i = 0; i < 8; i++) {
    const d = new Date(now);
    d.setDate(d.getDate() + i);
    d.setHours(h, m, 0, 0);
    const wd = (d.getDay() + 6) % 7;
    if (net.days.includes(wd) && d > now) return d;
  }
  return null;
}

function daysText(days) {
  const s = [...days].sort();
  if (s.length === 7) return "Every day";
  if (s.join() === "0,1,2,3,4") return "Weekdays";
  if (s.join() === "5,6") return "Weekends";
  return s.map((d) => DAY_NAMES[d]).join(", ");
}

function viewNets(main) {
  const o = station.recording;
  main.innerHTML = `<div class="page">
    <div id="banners">${bannerHtml()}</div>
    <div class="page-head"><h1>Nets</h1><span class="muted">During a net, every over goes into one recording with the silences cut out.</span><span class="grow"></span>
      <a class="btn primary rw" href="#/net/new">Add net</a></div>
    ${station.nets.length ? `<section class="net-grid" id="net-grid">${station.nets.map(netTileHtml).join("")}</section>`
      : `<div class="panel empty"><h3>No nets yet</h3><p>Add your morning or evening net and it's recorded as one file, ready to listen to or transcribe.</p></div>`}
    <section class="panel" aria-label="How nets are recorded">
      <div class="panel-head"><h2>How a net becomes one recording</h2></div>
      <div class="panel-body">
        <div class="form-grid">
          <div class="field"><label for="net-gap">Keep one recording going through silences up to <b class="mono" id="net-gap-text">${o.net_join_sec} s</b></label>
            <input type="range" id="net-gap" min="5" max="300" step="5" value="${o.net_join_sec}">
            <span class="hint">A longer silence ends the recording; the next over starts a new one.</span></div>
          <div class="field"><label for="net-overrun">If the net runs late, keep recording for up to</label>
            <select id="net-overrun">${[0, 15, 30, 60, 120].map((v) => `<option value="${v}" ${v === o.net_overrun_max_min ? "selected" : ""}>${v ? v + " minutes" : "No extra time"}</option>`).join("")}</select>
            <span class="hint">It stops sooner once the frequency goes quiet.</span></div>
        </div>
        <div class="row"><div class="text"><div class="title">Soft beep where each silence was cut</div><div class="sub">Two short quiet pips, so you can hear where one transmission ends and the next begins.</div></div>
          <button class="btn" type="button" data-act="beep">Preview beep</button>
          <button class="switch" type="button" role="switch" id="opt-beep" aria-checked="${o.gap_tone}" aria-label="Soft beep"></button></div>
        <div class="row"><div class="text"><div class="title">Even out loudness</div><div class="sub">Weak and strong stations play at the same volume.</div></div>
          <button class="switch" type="button" role="switch" id="opt-norm" aria-checked="${o.normalize}" aria-label="Even out loudness"></button></div>
        <div class="hint">These also apply to everyday recordings. A list of the time of every over is always saved next to each recording, for transcripts.</div>
        <div class="actions rw"><button class="btn primary" type="button" data-act="save-net-options">Save</button></div>
      </div>
    </section>
  </div>`;
  $("#net-gap").addEventListener("input", (e) => { $("#net-gap-text").textContent = `${e.target.value} s`; });
  updateNetsLive();
}

function netTileHtml(n) {
  const f = station.frequencies.find((x) => sameMhz(x.mhz, n.mhz));
  return `<article class="net-tile" data-net="${esc(n.id)}">
    <div style="display:flex;align-items:center;gap:10px"><span style="font-weight:600">${esc(n.name)}</span><span class="grow"></span><span class="tag" data-net-badge></span></div>
    <div style="display:flex;align-items:baseline;gap:10px"><span class="when">${esc(n.start)} – ${esc(n.end)}</span><span class="mono" style="color:var(--amber)">${fmtMhz(n.mhz)}</span></div>
    <div class="muted" style="font-size:12px">${esc(daysText(n.days))}${f && f.name ? " · " + esc(f.name) : ""}</div>
    <div style="display:flex;gap:8px;align-items:center;margin-top:4px">
      <a class="btn rw" href="#/net/${encodeURIComponent(n.id)}">${ICON.edit}Edit</a>
      <a class="btn ghost" href="#/recordings?freq=nets">Recordings</a>
      <span class="grow"></span>
      <button class="switch rw" type="button" role="switch" data-act="toggle-net" data-id="${esc(n.id)}" aria-checked="${n.enabled}" aria-label="Record ${esc(n.name)}"></button>
    </div>
  </article>`;
}

function updateNetsLive() {
  if (!status || !station) return;
  for (const n of station.nets) {
    const tile = $(`[data-net="${CSS.escape(n.id)}"]`);
    if (!tile) continue;
    const isLive = status.active_nets.includes(n.name);
    tile.classList.toggle("live", isLive);
    const badge = $("[data-net-badge]", tile);
    const next = nextStart(n);
    badge.className = "tag" + (isLive ? " amber" : "");
    badge.textContent = isLive ? "RECORDING NOW" : !n.enabled ? "OFF" : next ? `NEXT ${next.toLocaleDateString([], { weekday: "short" }).toUpperCase()} ${n.start}` : "";
  }
}

let netDraft = null;

function viewNetEditor(main) {
  const existing = station.nets.find((n) => n.id === route.arg);
  if (!station.frequencies.length) {
    main.innerHTML = `<div class="page narrow"><div class="panel empty"><h3>Add a frequency first</h3><p>A net is recorded on one of your frequencies.</p><p><a class="btn primary" href="#/frequency/new">Add frequency</a></p></div></div>`;
    return;
  }
  netDraft = existing ? clone(existing) : { id: "", name: "", mhz: station.frequencies[0].mhz, days: [0, 1, 2, 3, 4, 5, 6], start: "07:00", end: "08:00", enabled: true };
  const d = netDraft;
  main.innerHTML = `<div class="page narrow">
    <div class="page-head"><a class="btn icon" href="#/nets" aria-label="Back to nets">${ICON.left}</a><h1>${existing ? "Edit net" : "Add a net"}</h1></div>
    <section class="panel"><div class="panel-body">
      <div class="form-grid">
        <div class="field"><label for="net-name">Name</label><input type="text" id="net-name" value="${esc(d.name)}" placeholder="Morning net" maxlength="40"><span class="hint">Used for the recording's folder and file name.</span></div>
        <div class="field"><label for="net-freq">Frequency</label><select id="net-freq">${station.frequencies.map((f) => `<option value="${f.mhz}" ${sameMhz(f.mhz, d.mhz) ? "selected" : ""}>${fmtMhz(f.mhz)} MHz${f.name ? " · " + esc(f.name) : ""}</option>`).join("")}</select></div>
        <div class="field"><label for="net-start">Starts</label><input type="time" id="net-start" value="${esc(d.start)}"></div>
        <div class="field"><label for="net-end">Ends</label><input type="time" id="net-end" value="${esc(d.end)}"><span class="hint">An end before the start means it runs past midnight.</span></div>
      </div>
      <div class="field"><span class="label" id="days-label">Days</span>
        <div class="days" role="group" aria-labelledby="days-label">${DAY_NAMES.map((n, i) => `<button type="button" data-act="net-day" data-day="${i}" aria-pressed="${d.days.includes(i)}">${n}</button>`).join("")}</div></div>
      <div class="actions">
        <button class="btn primary big" type="button" data-act="save-net">${existing ? "Save" : "Add net"}</button>
        <a class="btn big" href="#/nets">Cancel</a><span class="grow"></span>
        ${existing ? `<button class="btn danger" type="button" data-act="delete-net">${ICON.trash}Delete net</button>` : ""}
      </div>
    </div></section>
  </div>`;
}

// ---------- frequency editor ----------

let freqDraft = null;
let planTimer = null;

function stopEditorTimers() { clearTimeout(planTimer); }

function viewFrequency(main) {
  const existing = route.arg && route.arg !== "new" ? station.frequencies.find((f) => f.id === route.arg) : null;
  freqDraft = existing ? clone(existing) : { id: "", mhz: Number(route.query.mhz) || null, name: "", modulation: "nfm", squelch: 5, record: true, ctcss_hz: null, notify: false };
  const d = freqDraft;
  const tones = ctcssTones.map((t) => `<option value="${t}" ${d.ctcss_hz != null && Math.abs(d.ctcss_hz - t) < 0.05 ? "selected" : ""}>${t.toFixed(1)} Hz</option>`).join("");
  main.innerHTML = `<div class="page narrow">
    <div class="page-head"><a class="btn icon" href="#/monitor" aria-label="Back to monitor">${ICON.left}</a><h1>${existing ? "Edit frequency" : "Add a frequency"}</h1></div>
    <section class="panel" aria-label="Where it sits in the receiver window"><div class="panel-body">
      <div style="display:flex;justify-content:space-between;gap:12px;flex-wrap:wrap"><span class="label">Receiver window</span><span class="hint" id="fit-text"></span></div>
      <div class="fit" id="fit"></div><div class="fit-scale" id="fit-scale"></div>
    </div></section>
    <section class="panel"><div class="panel-body">
      <div class="form-grid">
        <div class="field"><label for="f-mhz">Frequency (MHz)</label>
          <input type="text" id="f-mhz" inputmode="decimal" class="mono" style="height:56px;font-size:26px;color:var(--amber-hi)" value="${d.mhz ? fmtMhz(d.mhz) : ""}" placeholder="145.400" autocomplete="off">
          <span class="hint" id="f-mhz-hint">A repeater's output frequency, as on your radio.</span></div>
        <div class="field"><label for="f-name">Name</label><input type="text" id="f-name" maxlength="40" value="${esc(d.name)}" placeholder="Repeater A" style="height:56px;font-size:16px"><span class="hint">Shown on the dashboard. Optional.</span></div>
        <div class="field"><span class="label" id="mode-label">Mode</span>
          <div class="seg" role="group" aria-labelledby="mode-label"><button type="button" data-act="mode" data-mode="nfm" aria-pressed="${d.modulation !== "am"}">FM</button><button type="button" data-act="mode" data-mode="am" aria-pressed="${d.modulation === "am"}">AM</button></div>
          <span class="hint">FM for ham repeaters and simplex. AM for aviation.</span></div>
        <div class="field"><label for="f-sq">Squelch · <b id="f-sq-text">${d.squelch}</b></label>
          <input type="range" id="f-sq" min="0" max="9" step="1" value="${d.squelch}">
          <span class="hint">Like the SQL knob on a handheld: higher needs a stronger signal to start recording. 5 suits most repeaters. 0 is always open and records constantly.</span></div>
        <div class="field"><label for="f-ctcss">Only record with this CTCSS tone</label>
          <select id="f-ctcss"><option value="">Any signal</option>${tones}</select>
          <span class="hint">Ignores other stations on the same frequency. Use your repeater's tone.</span></div>
        <div class="field"><span class="label">Save recordings</span>
          <div style="display:flex;align-items:center;gap:12px;height:40px"><button class="switch" type="button" role="switch" id="f-record" aria-checked="${d.record}" aria-label="Save recordings"></button><span class="hint">Off: listen live only.</span></div></div>
        <div class="field"><span class="label">Notify me when it becomes active</span>
          <div style="display:flex;align-items:center;gap:12px;height:40px"><button class="switch" type="button" role="switch" id="f-notify" aria-checked="${!!d.notify}" aria-label="Notify me when it becomes active"></button><span class="hint">A Windows notification after 2 minutes of quiet, even with the window closed.</span></div></div>
      </div>
      <div class="actions">
        <button class="btn primary big" type="button" data-act="save-freq">${existing ? "Save" : "Add frequency"}</button>
        <a class="btn big" href="#/monitor">Cancel</a><span class="grow"></span>
        ${existing ? `<button class="btn danger" type="button" data-act="delete-freq">${ICON.trash}Delete</button>` : ""}
      </div>
      <div class="hint">Saving restarts the recorder, which takes about two seconds.</div>
    </div></section>
  </div>`;
  $("#f-mhz").addEventListener("input", () => { schedulePlan(); });
  $("#f-sq").addEventListener("input", (e) => { $("#f-sq-text").textContent = e.target.value; });
  schedulePlan(0);
  if (!existing) $("#f-mhz").focus();
}

function readMhz() {
  const v = $("#f-mhz").value.trim().replace(",", ".");
  const n = Number(v);
  return v && isFinite(n) && n > 0 ? n : null;
}

function schedulePlan(delay = 250) {
  clearTimeout(planTimer);
  planTimer = setTimeout(drawFit, delay);
}

async function drawFit() {
  const mhz = readMhz();
  const others = station.frequencies.filter((f) => f.id !== freqDraft.id).map((f) => f.mhz);
  const list = mhz ? [...others, mhz] : others;
  const fit = $("#fit");
  if (!fit) return;
  if (!list.length) {
    fit.innerHTML = "";
    $("#fit-scale").innerHTML = "";
    $("#fit-text").textContent = "Type a frequency";
    $("#fit-text").className = "hint";
    return;
  }
  let plan;
  try { plan = (await api("/api/plan", { method: "POST", body: { mhz: list } })).plan; } catch { return; }
  const lo = plan.low_mhz - 0.2, hi = plan.high_mhz + 0.2, span = hi - lo;
  const pos = (m) => Math.max(0, Math.min(100, ((m - lo) / span) * 100));
  const state = mhz ? plan.fits[plan.fits.length - 1] : null;
  fit.innerHTML = `<div class="window" style="left:${pos(plan.low_mhz)}%;right:${100 - pos(plan.high_mhz)}%"></div>`
    + others.map((m) => `<div class="tick" style="left:${pos(m)}%"></div>`).join("")
    + (mhz ? `<div class="tick new ${state === "outside" ? "bad" : ""}" style="left:${pos(mhz)}%"></div>` : "");
  $("#fit-scale").innerHTML = `<span>${plan.low_mhz.toFixed(2)}</span><span>centre ${fmtMhz(plan.centre_mhz)}</span><span>${plan.high_mhz.toFixed(2)}</span>`;
  const t = $("#fit-text");
  if (!mhz) { t.textContent = "Type a frequency"; t.className = "hint"; return; }
  if (others.some((m) => sameMhz(m, mhz))) { t.textContent = "You already have this frequency."; t.className = "hint bad"; return; }
  t.textContent = { good: "Fits in the receiver window", edge: "Fits, but near the edge where reception is weaker", outside: "Too far from your other frequencies for one dongle" }[state];
  t.className = "hint " + { good: "good", edge: "edge", outside: "bad" }[state];
}

// ---------- settings ----------

function viewSettings(main) {
  const s = station.settings, r = station.receiver, o = station.recording;
  const opt = (vals, cur, fmt) => vals.map((v) => `<option value="${v}" ${v === cur ? "selected" : ""}>${fmt(v)}</option>`).join("");
  const dir = status ? status.recordings_dir : "";
  main.innerHTML = `<div class="page narrow">
    <div id="banners">${bannerHtml()}</div>
    <div class="page-head"><h1>Settings</h1></div>
    <section class="panel" aria-label="Where recordings go"><div class="panel-head"><h2>Where recordings go</h2></div><div class="panel-body">
      <div style="display:flex;flex-wrap:wrap;gap:10px;align-items:center">
        <div class="mono" style="flex:1 1 320px;min-width:0;padding:11px 14px;border-radius:8px;border:1px solid var(--line-2);background:var(--inset);overflow:hidden;text-overflow:ellipsis;white-space:nowrap" title="${esc(dir)}">${esc(dir)}</div>
        <button class="btn" type="button" data-act="reveal">${ICON.folder}Open in Explorer</button>
        <button class="btn" type="button" data-act="pick-folder">Change…</button>
        ${s.recordings_dir ? `<button class="btn ghost" type="button" data-act="reset-folder">Use the default</button>` : ""}
      </div>
      <div class="hint" id="folder-state"></div>
      <div class="mono hint" style="line-height:1.7">145.400\\2026\\11\\12\\145.400_20261112_070201.mp3<br>Nets\\Morning net\\2026\\11\\12\\Morning net_20261112_070012.mp3<br>Each recording has a .segments.tsv next to it with the time of every over.</div>
      <div class="field"><label for="s-retention">Delete everyday recordings older than</label>
        <select id="s-retention" data-setting="retention_days" data-num>${opt([0, 30, 60, 90, 180, 365], s.retention_days, (v) => (v ? `${v} days` : "Never"))}</select>
        <span class="hint">Net recordings are never deleted automatically.</span></div>
    </div></section>

    <section class="panel" aria-label="Behaviour"><div class="panel-body" style="padding-top:4px;padding-bottom:4px">
      ${switchRow("autostart", "Start with Windows", info.autostart_available ? "Starts recording when you sign in, without opening the window." : "Available in the installed app.", s.autostart, !info.autostart_available)}
      ${switchRow("close_to_tray", "Keep recording when the window is closed", "The app stays in the system tray. Right-click its icon to quit.", s.close_to_tray)}
      ${switchRow("local_time", "Use local time in file names", "Off: UTC.", s.local_time)}
      <div class="row"><div class="text"><div class="title">Appearance</div><div class="sub">The waterfall stays dark in every theme. Also on the button at the top right.</div></div>
        <select id="s-theme" aria-label="Theme">${opt(["system", "light", "dark"], s.theme || "system", (v) => ({ system: "Same as Windows", light: "Light", dark: "Dark" })[v])}</select></div>
      <div class="row"><div class="text"><div class="title">Warn me if nothing is heard for</div><div class="sub">Usually means the antenna or dongle has a problem.</div></div>
        <select data-setting="silence_warning_hours" data-num aria-label="Silence warning">${opt([0, 6, 12, 24, 48, 72], s.silence_warning_hours, (v) => (v ? `${v} hours` : "Don't warn"))}</select></div>
    </div></section>

    ${client.remote ? "" : `<section class="panel" aria-label="Other devices on your network"><div class="panel-head"><h2>Other devices on your network</h2><span class="muted">Phones, tablets and other PCs on the same Wi-Fi</span></div><div class="panel-body">
      <div class="field"><label for="r-pw">Password</label>
        <div style="display:flex;gap:10px;flex-wrap:wrap"><input type="password" id="r-pw" autocomplete="new-password" placeholder="At least 6 characters" style="flex:1 1 220px">
          <button class="btn" type="button" data-act="remote-password">Set password</button></div>
        <span class="hint" id="r-pw-state"></span></div>
      <div class="row"><div class="text"><div class="title">Allow other devices on my network</div><div class="sub">They sign in with the password. Windows may ask once whether to allow Frequency Recorder through the firewall: choose <b>Private networks</b>.</div></div>
        <button class="switch" type="button" role="switch" data-act="remote-switch" id="r-on" aria-checked="${!!s.remote_access}" aria-label="Allow other devices on my network"></button></div>
      <div class="row"><div class="text"><div class="title">Watch and listen only</div><div class="sub">Other devices see the waterfall, listen and play recordings, but can't change anything.</div></div>
        ${`<button class="switch" type="button" role="switch" data-act="setting-switch" data-key="remote_read_only" aria-checked="${!!s.remote_read_only}" aria-label="Watch and listen only"></button>`}</div>
      <div id="r-status" class="hint"></div>
    </div></section>`}

    <section class="panel" aria-label="Everyday recordings"><div class="panel-head"><h2>Everyday recordings</h2></div><div class="panel-body">
      <div class="form-grid">
        <div class="field"><label for="s-join">Put overs in the same recording when they're less than <b class="mono" id="s-join-text">${o.clip_join_sec} s</b> apart</label>
          <input type="range" id="s-join" min="0" max="60" step="1" value="${o.clip_join_sec}" data-recording="clip_join_sec" data-num>
          <span class="hint">0: every over is its own recording. Higher: a back-and-forth becomes one recording.</span></div>
        <div class="field"><label for="s-min">Ignore transmissions shorter than</label>
          <select id="s-min" data-recording="min_clip_sec" data-num>${opt([0, 0.5, 1, 2], o.min_clip_sec, (v) => (v ? `${v} s` : "Keep everything"))}</select>
          <span class="hint">Drops kerchunks and squelch blips.</span></div>
      </div>
      <div class="hint">Beeps between overs and loudness evening are set on the <a href="#/nets">Nets</a> page.</div>
    </div></section>

    <section class="panel" aria-label="Receiver"><div class="panel-head"><h2>Receiver</h2><span class="muted">${status && status.plan ? `Tuned to ${fmtMhz(status.plan.centre_mhz)} MHz automatically` : ""}</span></div><div class="panel-body">
      <div class="form-grid">
        <div class="field"><label for="r-gain">Gain · <b id="r-gain-text">${r.gain_db}</b> dB</label><input type="range" id="r-gain" min="0" max="49.6" step="0.1" value="${r.gain_db}">
          <span class="hint">Higher hears weaker stations but adds noise. 25–35 dB suits most antennas.</span></div>
        <div class="field"><label for="r-ppm">Frequency correction (ppm)</label><input type="number" id="r-ppm" min="-200" max="200" step="1" value="${r.correction_ppm}">
          <span class="hint">0 for an RTL-SDR Blog V3 or V4.</span></div>
        <div class="field"><label for="r-index">Dongle number</label><input type="number" id="r-index" min="0" max="9" step="1" value="${r.device_index}">
          <span class="hint">0 unless more than one is plugged in.</span></div>
      </div>
      <div class="actions"><button class="btn" type="button" data-act="save-receiver">Save receiver settings</button><span class="hint">Restarts the recorder.</span></div>
    </div></section>

    <section class="panel"><div class="panel-body">
      <details id="engine-details"><summary>Recorder log and settings file</summary>
        <div style="display:flex;flex-direction:column;gap:10px;margin-top:12px">
          <pre class="log" id="engine-log">Loading…</pre>
          <details><summary>Settings file the app wrote for the recorder</summary><pre class="log" id="engine-conf"></pre></details>
        </div></details>
      <div class="footer-credit">Frequency Recorder records with <a href="https://github.com/rtl-airband/RTLSDR-Airband" target="_blank" rel="noopener">RTLSDR-Airband</a> (GPL). Built by Ruchir · VU22PR, India · <a href="https://www.giar.org" target="_blank" rel="noopener">GIAR</a>. Recording radio traffic is regulated differently in each country: it's up to you to follow the rules where you are.</div>
    </div></section>
  </div>`;
  $("#folder-state").textContent = status ? `${status.recordings_dir_exists ? "Folder ready" : "Folder will be created"} · recordings use ${bytes(status.disk.used_bytes)}${status.disk.free_bytes != null ? " · " + bytes(status.disk.free_bytes) + " free on this drive" : ""}` : "";
  $("#r-gain").addEventListener("input", (e) => { $("#r-gain-text").textContent = e.target.value; });
  $("#s-theme").addEventListener("change", (e) => setTheme(e.target.value));
  if (!client.remote) refreshRemote();
  $("#s-join").addEventListener("input", (e) => { $("#s-join-text").textContent = `${e.target.value} s`; });
  $("#engine-details").addEventListener("toggle", async (e) => {
    if (!e.target.open) return;
    try {
      const log = await api("/api/engine/log");
      $("#engine-log").textContent = log.lines.slice(-300).join("\n") || "Nothing logged yet.";
      $("#engine-conf").textContent = log.config || "Not written yet.";
    } catch (err) { $("#engine-log").textContent = err.message; }
  });
}

/** Fills in the network-access part of Settings. */
async function refreshRemote() {
  let r;
  try { r = await api("/api/remote"); } catch { return; }
  const pw = $("#r-pw-state"), st = $("#r-status");
  if (!pw) return;
  pw.textContent = r.has_password ? "A password is set. Setting a new one signs every device out." : "Set a password first.";
  $("#r-on").setAttribute("aria-checked", r.enabled);
  if (r.error) { st.className = "hint bad"; st.textContent = r.error; return; }
  if (r.running && r.url) {
    st.className = "hint";
    st.innerHTML = `On other devices, open <span class="remote-url">${esc(r.url)}</span>`;
  } else if (r.running) {
    st.className = "hint edge";
    st.textContent = `On, at port ${r.port}, but this PC doesn't seem to be on a network.`;
  } else {
    st.className = "hint";
    st.textContent = r.enabled && !r.has_password ? "Set a password to turn it on." : "Off: only this PC can open the app.";
  }
}

function switchRow(key, title, sub, on, disabled = false) {
  return `<div class="row"><div class="text"><div class="title">${esc(title)}</div><div class="sub">${esc(sub)}</div></div>
    <button class="switch" type="button" role="switch" data-act="setting-switch" data-key="${key}" aria-checked="${!!on}" aria-label="${esc(title)}" ${disabled ? "disabled" : ""}></button></div>`;
}

// ---------- actions ----------

let squelchTimer = null;

/** Shows a frequency's new squelch on its tile and the trace; `save` sends it (live, no restart). */
function setTileSquelch(f, sq, save) {
  f.squelch = sq;
  const id = CSS.escape(f.id);
  const slider = $(`[data-squelch-id="${id}"]`);
  if (slider) slider.value = sq;
  const num = $(`[data-squelch="${id}"]`);
  if (num) num.textContent = sq;
  const warn = $(`[data-open-warning="${id}"]`);
  if (warn) warn.classList.toggle("hidden", !(sq === 0 && f.record));
  wfTrace();
  clearTimeout(squelchTimer);
  if (save) squelchTimer = setTimeout(() => saveStation(clone(station), `Squelch ${sq} on ${label(f)}`), 300);
}

// tile squelch sliders: the number follows the slider, the station is saved once it stops moving
document.addEventListener("input", (e) => {
  const id = client.read_only ? null : e.target.dataset.squelchId;
  if (!id) return;
  const f = station.frequencies.find((x) => x.id === id);
  if (!f) return;
  setTileSquelch(f, Number(e.target.value), false);
  squelchTimer = setTimeout(() => saveStation(clone(station), `Squelch ${f.squelch} on ${label(f)}`), 900);
});

const actions = {
  async listen(el) {
    const fs = status && status.frequencies.find((x) => x.id === el.dataset.id);
    if (!fs) return;
    if (live && live.port === fs.port) stopLive(); else startLive(fs.port);
    updateMonitorLive();
  },
  async day(el) {
    const delta = Number(el.dataset.delta);
    recDay = delta ? startOfDay(new Date(recDay.getTime() + delta * 86400000 + 3600000)) : startOfDay(new Date());
    loadDay();
  },
  "rec-filter"(el) { $("#rec-filters").dataset.filter = el.dataset.value; drawDay(); },
  "play-at"(el) {
    const rec = recList.find((r) => r.path === el.dataset.path);
    if (rec) playRecording(rec, Number(el.dataset.offset) || 0.01);
  },
  async reveal(el) {
    try { await api("/api/reveal", { method: "POST", body: el.dataset.path ? { path: el.dataset.path } : {} }); } catch (e) { toast(e.message, true); }
  },
  async "delete-rec"(el) {
    const path = el.dataset.path;
    if (!confirm(`Delete this recording for good?\n\n${path}`)) return;
    try {
      await api(`/api/recordings/${encodePath(path)}`, { method: "DELETE" });
      if (playing && playing.path === path) $("#player-close").click();
      recList = recList.filter((r) => r.path !== path);
      recent = recent.filter((r) => r.path !== path);
      if (route.name === "recordings") drawDay(); else fillMonitorRecordings();
      toast("Deleted");
    } catch (e) { toast(e.message, true); }
  },
  beep() { previewBeep(); },
  async "save-net-options"() {
    const next = clone(station);
    next.recording.net_join_sec = Number($("#net-gap").value);
    next.recording.net_overrun_max_min = Number($("#net-overrun").value);
    next.recording.gap_tone = $("#opt-beep").getAttribute("aria-checked") === "true";
    next.recording.normalize = $("#opt-norm").getAttribute("aria-checked") === "true";
    await saveStation(next);
  },
  async "toggle-net"(el) {
    const next = clone(station);
    const n = next.nets.find((x) => x.id === el.dataset.id);
    n.enabled = !n.enabled;
    if (await saveStation(next, n.enabled ? `${n.name} will be recorded` : `${n.name} won't be recorded`)) render();
  },
  "net-day"(el) {
    const day = Number(el.dataset.day);
    const on = el.getAttribute("aria-pressed") !== "true";
    el.setAttribute("aria-pressed", on);
    netDraft.days = on ? [...netDraft.days, day].sort() : netDraft.days.filter((d) => d !== day);
  },
  async "save-net"() {
    const d = netDraft;
    d.name = $("#net-name").value.trim();
    d.mhz = Number($("#net-freq").value);
    d.start = $("#net-start").value;
    d.end = $("#net-end").value;
    const next = clone(station);
    const i = next.nets.findIndex((n) => d.id && n.id === d.id);
    if (i >= 0) next.nets[i] = d; else next.nets.push(d);
    if (await saveStation(next, i >= 0 ? "Net saved" : "Net added")) location.hash = "#/nets";
  },
  async "delete-net"() {
    if (!confirm(`Delete ${netDraft.name}? Its recordings stay in the folder.`)) return;
    const next = clone(station);
    next.nets = next.nets.filter((n) => n.id !== netDraft.id);
    if (await saveStation(next, "Net deleted")) location.hash = "#/nets";
  },
  mode(el) {
    freqDraft.modulation = el.dataset.mode;
    $$('[data-act="mode"]').forEach((b) => b.setAttribute("aria-pressed", b === el));
  },
  async "save-freq"() {
    const mhz = readMhz();
    if (!mhz) { $("#f-mhz").classList.add("invalid"); $("#f-mhz-hint").textContent = "Type the frequency in MHz, like 145.400."; $("#f-mhz-hint").className = "hint bad"; $("#f-mhz").focus(); return; }
    const d = freqDraft;
    const oldMhz = d.mhz;
    d.mhz = mhz;
    d.name = $("#f-name").value.trim();
    d.squelch = Number($("#f-sq").value);
    d.ctcss_hz = $("#f-ctcss").value ? Number($("#f-ctcss").value) : null;
    d.record = $("#f-record").getAttribute("aria-checked") === "true";
    d.notify = $("#f-notify").getAttribute("aria-checked") === "true";
    const next = clone(station);
    const i = next.frequencies.findIndex((f) => d.id && f.id === d.id);
    if (i >= 0) {
      next.frequencies[i] = d;
      if (oldMhz) next.nets.forEach((n) => { if (sameMhz(n.mhz, oldMhz)) n.mhz = mhz; });
    } else next.frequencies.push(d);
    if (await saveStation(next, i >= 0 ? "Saved" : `${fmtMhz(mhz)} MHz added. Recording starts in a few seconds.`)) location.hash = "#/monitor";
  },
  async "delete-freq"() {
    const d = freqDraft;
    const nets = station.nets.filter((n) => sameMhz(n.mhz, d.mhz));
    const extra = nets.length ? `\n\nThe ${nets.map((n) => n.name).join(", ")} net${nets.length > 1 ? "s" : ""} on it will be deleted too.` : "";
    if (!confirm(`Stop recording ${label(d)}? Its recordings stay in the folder.${extra}`)) return;
    const next = clone(station);
    next.frequencies = next.frequencies.filter((f) => f.id !== d.id);
    next.nets = next.nets.filter((n) => !sameMhz(n.mhz, d.mhz));
    if (await saveStation(next, "Frequency removed")) location.hash = "#/monitor";
  },
  async "setting-switch"(el) {
    const next = clone(station);
    next.settings[el.dataset.key] = !next.settings[el.dataset.key];
    if (await saveStation(next)) el.setAttribute("aria-checked", next.settings[el.dataset.key]);
  },
  async "remote-password"() {
    const input = $("#r-pw");
    try {
      await api("/api/remote/password", { method: "POST", body: { password: input.value } });
      input.value = "";
      toast("Password set");
      refreshRemote();
    } catch (e) { toast(e.message, true); }
  },
  async "remote-switch"(el) {
    const on = el.getAttribute("aria-checked") !== "true";
    if (on) {
      const r = await api("/api/remote").catch(() => null);
      if (r && !r.has_password) { toast("Set a password first", true); $("#r-pw").focus(); return; }
    }
    const next = clone(station);
    next.settings.remote_access = on;
    if (await saveStation(next, on ? "Other devices can now sign in" : "Only this PC can open the app now")) {
      el.setAttribute("aria-checked", on);
      setTimeout(refreshRemote, 300);
    }
  },
  async "pick-folder"() {
    try {
      const res = await api("/api/pick-folder", { method: "POST" });
      if (!res.path) return;
      const next = clone(station);
      next.settings.recordings_dir = res.path;
      if (await saveStation(next, "New recordings go to the new folder. Existing ones stay where they are.")) { await pollStatus(); render(); }
    } catch (e) { toast(e.message, true); }
  },
  async "reset-folder"() {
    const next = clone(station);
    next.settings.recordings_dir = "";
    if (await saveStation(next)) { await pollStatus(); render(); }
  },
  async "save-receiver"() {
    const next = clone(station);
    next.receiver.gain_db = Number($("#r-gain").value);
    next.receiver.correction_ppm = Math.round(Number($("#r-ppm").value) || 0);
    next.receiver.device_index = Math.max(0, Math.round(Number($("#r-index").value) || 0));
    await saveStation(next);
  },
};

document.addEventListener("click", (e) => {
  const play = e.target.closest("[data-play]");
  if (play) {
    const path = play.dataset.play;
    const rec = [...recList, ...recent].find((r) => r.path === path);
    if (rec) playRecording(rec);
    return;
  }
  const sw = e.target.closest('.switch:not([data-act])');
  if (sw && !sw.disabled) { sw.setAttribute("aria-checked", sw.getAttribute("aria-checked") !== "true"); return; }
  const el = e.target.closest("[data-act]");
  if (el && actions[el.dataset.act] && !el.disabled) actions[el.dataset.act](el);
});

document.addEventListener("change", async (e) => {
  const el = e.target;
  if (el.dataset.setting || el.dataset.recording) {
    const next = clone(station);
    const value = el.dataset.num !== undefined ? Number(el.value) : el.value;
    if (el.dataset.setting) next.settings[el.dataset.setting] = value; else next.recording[el.dataset.recording] = value;
    await saveStation(next);
  }
});

$("#mode-switch").addEventListener("click", async (e) => {
  const b = e.target.closest("button");
  if (!b || !status) return;
  const enable = b.dataset.mode === "auto";
  if (enable === status.recording_enabled) return;
  if (!enable && !confirm("Switch to listen only? The waterfall and live listening keep working, but nothing is saved until you switch back to auto-record.")) return;
  try {
    await api("/api/recording", { method: "POST", body: { enabled: enable } });
    toast(enable ? "Auto-record on: every transmission is recorded" : "Listen only: nothing is saved");
    pollStatus();
  } catch (err) { toast(err.message, true); }
});

// ---------- theme ----------

const THEMES = ["system", "light", "dark"];
const THEME_ICON = {
  system: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg>',
  light: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>',
  dark: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z"/></svg>',
};
const prefersLight = matchMedia("(prefers-color-scheme: light)");

function themeChoice() {
  if (client.remote) {
    try { return localStorage.getItem("theme") || "system"; } catch { return "system"; }
  }
  return (station && station.settings.theme) || "system";
}

function applyTheme() {
  const t = themeChoice();
  document.documentElement.dataset.theme = t === "system" ? (prefersLight.matches ? "light" : "dark") : t;
  try { localStorage.setItem("theme", t); } catch { /* private window */ }
  const btn = $("#btn-theme");
  btn.innerHTML = THEME_ICON[t];
  const name = { system: "Same as Windows", light: "Light", dark: "Dark" }[t];
  btn.title = `Theme: ${name} (click to change)`;
  btn.setAttribute("aria-label", `Theme: ${name}. Click to change.`);
  const sel = $("#s-theme");
  if (sel) sel.value = t;
}

async function setTheme(t) {
  if (client.remote) {
    // another device picks its own theme, without changing the PC's
    try { localStorage.setItem("theme", t); } catch { /* private window */ }
    return applyTheme();
  }
  const next = clone(station);
  next.settings.theme = t;
  station.settings.theme = t;
  applyTheme();
  await saveStation(next, "");
}

prefersLight.addEventListener("change", applyTheme);
$("#btn-theme").addEventListener("click", () => setTheme(THEMES[(THEMES.indexOf(themeChoice()) + 1) % THEMES.length]));
$("#btn-signout").addEventListener("click", async () => {
  try { await fetch("/api/logout", { method: "POST" }); } catch { /* signed out anyway */ }
  location.href = "/login";
});

// ---------- start ----------

(async function init() {
  try {
    const res = await api("/api/station");
    station = res.station;
    ctcssTones = res.ctcss_tones;
    info = res;
    client = res.client || client;
    document.documentElement.classList.toggle("remote", client.remote);
    document.documentElement.classList.toggle("read-only", client.read_only);
    applyTheme();
  } catch (e) {
    $("#main").innerHTML = `<div class="page"><div class="banner">${esc(e.message)}</div></div>`;
    return;
  }
  await pollStatus();
  window.addEventListener("hashchange", render);
  render();
  setInterval(pollStatus, 1000);
  setInterval(() => { if (route.name === "monitor") loadRecent().then(fillMonitorRecordings); }, 30000);
})();
