"use strict";

const $ = (sel) => document.querySelector(sel);
const grid = $("#grid");
const ctx = $("#ctx");

const state = {
  items: [],
  selected: null,
  sort: "added",
  query: "",
  channel: null, // channel filter from the Channels dropdown
  coverTarget: null,
  selecting: false, // selection mode: pick several videos for bulk actions
  picked: new Set(),
  lastPicked: null, // anchor for Shift + click ranges
  missingOnly: false, // "Missing files" chip
  view: "home", // "home" | "collections" | "collection" (one open collection)
  collectionId: null,
  colSort: "updated", // sort chips on the Collections page
  collections: [],
};

const currentCollection = () => state.collections.find((c) => c.id === state.collectionId);

const ICONS = {
  play: '<svg viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>',
  more: '<svg viewBox="0 0 24 24"><path d="M12 8c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm0 2c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm0 6c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z"/></svg>',
  film: '<svg viewBox="0 0 24 24"><path d="M18 4l2 4h-3l-2-4h-2l2 4h-3l-2-4H8l2 4H7L5 4H4c-1.1 0-1.99.9-1.99 2L2 18c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V4h-4z"/></svg>',
  plus: '<svg viewBox="0 0 24 24"><path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z"/></svg>',
  check: '<svg viewBox="0 0 24 24"><path d="M9 16.17 4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z"/></svg>',
  channels: '<svg viewBox="0 0 24 24"><path d="M16 11c1.66 0 2.99-1.34 2.99-3S17.66 5 16 5c-1.66 0-3 1.34-3 3s1.34 3 3 3zm-8 0c1.66 0 2.99-1.34 2.99-3S9.66 5 8 5C6.34 5 5 6.34 5 8s1.34 3 3 3zm0 2c-2.33 0-7 1.17-7 3.5V19h14v-2.5c0-2.33-4.67-3.5-7-3.5zm8 0c-.29 0-.62.02-.97.05 1.16.84 1.97 1.97 1.97 3.45V19h6v-2.5c0-2.33-4.67-3.5-7-3.5z"/></svg>',
  list: '<svg viewBox="0 0 24 24"><path d="M19 9H2v2h17V9zm0-4H2v2h17V5zM2 15h13v-2H2v2zm15-2v6l5-3-5-3z"/></svg>',
  collection: '<svg viewBox="0 0 24 24"><path d="M4 6H2v14c0 1.1.9 2 2 2h14v-2H4V6zm16-4H8c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V4c0-1.1-.9-2-2-2zm-8 12.5v-9l6 4.5-6 4.5z"/></svg>',
  chevron: '<svg class="chev" viewBox="0 0 24 24"><path d="m12 15.7-6-6 1.4-1.4 4.6 4.6 4.6-4.6L18 9.7z"/></svg>',
};

// ---------------------------------------------------------------- api

async function api(path, body) {
  const res = await fetch(path, body === undefined ? {} : {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `Request failed (${res.status})`);
  if (data.collections) state.collections = data.collections;
  if (data.library) setItems(data.library);
  else if (data.collections) render();
  return data;
}

function setItems(items) {
  state.items = items;
  if (state.view === "collection" && !currentCollection()) state.view = "collections";
  if (state.selected && !items.some((i) => i.id === state.selected)) state.selected = null;
  if (state.channel && !items.some((i) => i.channel === state.channel)) state.channel = null;
  if (state.missingOnly && items.every((i) => i.exists)) state.missingOnly = false;
  render();
  queueAutoThumbs();
}

// ---------------------------------------------------------------- formatting

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

function fmtDuration(sec) {
  if (!sec || !isFinite(sec)) return "";
  sec = Math.round(sec);
  const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
  const pad = (n) => String(n).padStart(2, "0");
  return h ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

function fmtSize(bytes) {
  if (bytes == null) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (bytes >= 1024 && i < units.length - 1) { bytes /= 1024; i++; }
  return `${bytes.toFixed(i >= 3 ? 1 : 0)} ${units[i]}`;
}

function timeAgo(ts) {
  const diff = Date.now() / 1000 - ts;
  const steps = [[31536000, "year"], [2592000, "month"], [604800, "week"], [86400, "day"], [3600, "hour"], [60, "minute"]];
  for (const [secs, name] of steps) {
    const n = Math.floor(diff / secs);
    if (n >= 1) return `${n} ${name}${n > 1 ? "s" : ""} ago`;
  }
  return "just now";
}

function avatarColor(name) {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) % 360;
  return `hsl(${h} 55% 42%)`;
}

// ---------------------------------------------------------------- render

function visibleItems() {
  const q = state.query.toLowerCase();
  // Inside a collection: only its videos, and "added" means the collection's own order.
  const col = state.view === "collection" && currentCollection();
  const order = col ? new Map(col.videos.map((id, n) => [id, n])) : null;
  const list = state.items.filter((i) =>
    (!order || order.has(i.id)) &&
    (!state.channel || i.channel === state.channel) &&
    (!state.missingOnly || !i.exists) &&
    (!q || `${i.title} ${i.channel} ${i.path}`.toLowerCase().includes(q)));
  const by = {
    added: order ? (a, b) => order.get(a.id) - order.get(b.id) : (a, b) => b.added - a.added,
    played: (a, b) => (b.lastPlayed || 0) - (a.lastPlayed || 0) || b.added - a.added,
    title: (a, b) => a.title.localeCompare(b.title, undefined, { numeric: true }),
    channel: (a, b) => a.channel.localeCompare(b.channel) || a.title.localeCompare(b.title, undefined, { numeric: true }),
  }[state.sort];
  return list.sort(by);
}

// Channel photo, or the channel's first letter on a color picked from its name.
function avatarInner(name, src) {
  return src ? `<img src="${esc(src)}" alt="" draggable="false">` : esc((name[0] || "?").toUpperCase());
}

const avatarUrl = (item) => item.channelAvatar && `/avatars/${encodeURIComponent(item.channelAvatar)}`;

function cardHtml(item) {
  const thumb = item.cover
    ? `<img src="/covers/${encodeURIComponent(item.cover)}" alt="" draggable="false" loading="lazy" decoding="async">`
    : `<div class="placeholder ${item.exists && !item.autoTried ? "loading" : ""}">${ICONS.film}<span>${esc(item.ext)}</span></div>`;
  const meta = [fmtSize(item.size), `added ${timeAgo(item.added)}`].filter(Boolean).join(" • ");
  return `
    <article class="card ${item.exists ? "" : "is-missing"} ${item.watched ? "is-watched" : ""}" data-id="${item.id}" title="${esc(item.path)}">
      <div class="thumb">
        ${thumb}
        <span class="pick" aria-hidden="true">${ICONS.check}</span>
        ${item.duration ? `<span class="badge">${fmtDuration(item.duration)}</span>` : ""}
        ${item.watched ? `<div class="watched-bar"></div>` : ""}
        ${item.exists ? "" : `<span class="badge missing" data-action="locate" title="Click to locate the file">File not found</span>`}
        <button class="play-btn" data-action="play" aria-label="Play">${ICONS.play}</button>
      </div>
      <div class="info">
        <div class="avatar" data-action="channel" title="Edit channel" style="background:${avatarColor(item.channel)}">${avatarInner(item.channel, avatarUrl(item))}</div>
        <div class="text">
          <h3 class="title">${esc(item.title)}</h3>
          <div class="channel" data-action="channel" title="Edit channel">${esc(item.channel)}</div>
          <div class="meta">${esc(meta)}</div>
        </div>
        <button class="more" data-action="menu" aria-label="More actions">${ICONS.more}</button>
      </div>
    </article>`;
}

const elementFrom = (html) => {
  const t = document.createElement("template");
  t.innerHTML = html.trim();
  return t.content.firstElementChild;
};

// Cards are reused while their content is unchanged, so a re-render never
// reloads images or restarts hover effects on the card under the mouse.
const cardCache = new Map(); // id -> { html, el }
let addCardEl = null;

// ---- motion: small, quick animations so the grid doesn't just jump

const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)");
const EASE_OUT = "cubic-bezier(.2, .7, .2, 1)";
let quietCards = false; // while switching views the whole page fades in instead
const cardKey = (el) => el.dataset.id || el.dataset.cid || el.dataset.action || el.dataset.col || null;

// Puts nodes into a container, leaving the DOM alone when nothing moved.
// Cards that stay slide from where they were to their new place (sorting, filtering,
// deleting); new cards fade and rise in, one after another. Only cards on screen move,
// so a big library stays quick.
function placeNodes(container, nodes) {
  const current = [...container.children];
  if (current.length === nodes.length && nodes.every((n, i) => current[i] === n)) return;
  const animate = !reduceMotion.matches && !quietCards && !container.hidden && container.isConnected;
  const before = new Map();
  if (animate) {
    for (const el of current) {
      const key = cardKey(el);
      if (key) before.set(key, el.getBoundingClientRect()); // where it is now, mid-animation included
    }
    // A slide still running starts over from where the card is; fade-ins keep going.
    for (const el of current) el.getAnimations().forEach((a) => a.id === "slide" && a.cancel());
  }
  container.replaceChildren(...nodes);
  if (!animate) return;
  const top = -120, bottom = innerHeight + 120;
  let entering = 0;
  for (const el of nodes) {
    const key = cardKey(el);
    if (!key) continue;
    const to = el.getBoundingClientRect();
    const from = before.get(key);
    if (from) {
      const dx = from.left - to.left, dy = from.top - to.top;
      const onScreen = (to.bottom > top && to.top < bottom) || (from.bottom > top && from.top < bottom);
      if (onScreen && (Math.abs(dx) > 1 || Math.abs(dy) > 1)) {
        el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }],
          { id: "slide", duration: 280, easing: EASE_OUT });
      }
    } else if (to.bottom > top && to.top < bottom) {
      el.animate([{ opacity: 0, transform: "translateY(14px) scale(.97)" }, { opacity: 1, transform: "none" }],
        { duration: 260, delay: Math.min(entering++ * 35, 280), easing: EASE_OUT, fill: "backwards" });
    }
  }
}

// The page under the bars fades and rises in (switching Home / Collections / a collection).
function enterView() {
  if (reduceMotion.matches) return;
  const shown = [$("#colHeader"), $("#chips"), state.view === "collections" ? colGrid : grid].filter((el) => !el.hidden);
  shown.forEach((el, i) => el.animate(
    [{ opacity: 0, transform: "translateY(10px)" }, { opacity: 1, transform: "none" }],
    { duration: 240, delay: i * 30, easing: EASE_OUT, fill: "backwards" }));
}

function render() {
  document.body.dataset.view = state.view;
  const tab = state.view === "collection" ? "collections" : state.view;
  document.querySelectorAll(".view-tabs [data-view]").forEach((b) => b.classList.toggle("active", b.dataset.view === tab));
  $("#search").placeholder = state.view === "collections" ? "Search collections" : "Search";
  $(".chip[data-sort='added']").textContent = state.view === "collection" ? "Collection order" : "Recently added";
  grid.hidden = state.view === "collections";
  colGrid.hidden = state.view !== "collections";
  renderCollectionHeader();
  if (state.view === "collections") {
    renderCollections();
    $("#missingChip").hidden = true;
    return updateMenu();
  }

  const items = visibleItems();
  const nodes = items.map((item) => {
    const html = cardHtml(item);
    let cached = cardCache.get(item.id);
    if (!cached || cached.html !== html) {
      cached = { html, el: elementFrom(html) };
      cardCache.set(item.id, cached);
    }
    cached.el.classList.toggle("selected", !state.selecting && item.id === state.selected);
    cached.el.classList.toggle("picked", state.picked.has(item.id));
    return cached.el;
  });
  for (const id of cardCache.keys()) if (!state.items.some((i) => i.id === id)) cardCache.delete(id);
  // Bulk actions only touch what you can see.
  const visible = new Set(items.map((i) => i.id));
  for (const id of state.picked) if (!visible.has(id)) state.picked.delete(id);

  const filtered = state.query || state.channel || state.selecting || state.missingOnly;
  addCardEl ||= elementFrom(`
    <article class="card add-card" data-action="add">
      <div class="thumb"><div class="plus">${ICONS.plus}</div><span>Add video</span></div>
      <div class="info">
        <div class="avatar"></div>
        <div class="text"><h3 class="title">Add a new video</h3><div class="channel">Pick files from your drives</div></div>
      </div>
    </article>`);
  if (!filtered) nodes.push(addCardEl);
  const inCollection = state.view === "collection";
  if (inCollection && !currentCollection()?.videos.length) {
    nodes.push(elementFrom(`<p class="empty">This collection is empty. On <b>Home</b>, use ⋮ → <b>Save to collection</b>, or <b>Select</b> several videos → <b>Add to collection</b>. Videos you add here with <b>+</b> go straight into it.</p>`));
  } else if (!state.items.length) {
    nodes.push(elementFrom(`<p class="empty">Your library is empty. Click <b>+</b> to add your first video.</p>`));
  } else if (!items.length) {
    nodes.push(elementFrom(`<p class="empty">No videos match “${esc(state.query)}”${state.channel ? ` in ${esc(state.channel)}` : ""}.</p>`));
  }
  placeNodes(grid, nodes);

  const n = inCollection ? currentCollection()?.videos.length ?? 0 : state.items.length;
  $("#count").textContent = items.length < n ? `${items.length} of ${n} videos` : plural(n, "video");
  const missing = state.items.filter((i) => !i.exists).length;
  const missingChip = $("#missingChip");
  missingChip.hidden = !missing;
  missingChip.classList.toggle("active", state.missingOnly);
  missingChip.textContent = `Missing files (${missing})`;

  renderChannelToggle();
  updateMenu();
}

// ---------------------------------------------------------------- collections (playlists of videos)

const colGrid = $("#colGrid");
const colCtx = $("#colCtx");
const colCache = new Map(); // id -> { html, el }
let createColEl = null;

const itemsById = () => new Map(state.items.map((i) => [i.id, i]));

function colCardHtml(col, byId) {
  const vids = col.videos.map((id) => byId.get(id)).filter(Boolean);
  // Your own cover, or else the first video's thumbnail.
  const cover = col.cover || vids.find((v) => v.cover)?.cover;
  const thumb = cover
    ? `<img src="/covers/${encodeURIComponent(cover)}" alt="" draggable="false" loading="lazy" decoding="async">`
    : `<div class="col-empty-thumb">${ICONS.collection}<span>${vids.length ? "No covers yet" : "No videos yet"}</span></div>`;
  return `
    <article class="card col-card" data-cid="${col.id}">
      <div class="stack">
        <div class="thumb">${thumb}<span class="badge">${ICONS.list}${plural(vids.length, "video")}</span></div>
      </div>
      <div class="info">
        <div class="text">
          <h3 class="title">${esc(col.name)}</h3>
          <div class="meta">Collection • updated ${timeAgo(col.updated)}</div>
          <div class="view-all">View full collection</div>
        </div>
        <button class="more" data-colmenu aria-label="More actions">${ICONS.more}</button>
      </div>
    </article>`;
}

function renderCollections() {
  const q = state.query.toLowerCase();
  const byId = itemsById();
  const by = {
    updated: (a, b) => b.updated - a.updated,
    created: (a, b) => b.created - a.created,
    played: (a, b) => (b.lastPlayed || 0) - (a.lastPlayed || 0) || b.updated - a.updated,
    name: (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }),
    size: (a, b) => b.videos.length - a.videos.length || b.updated - a.updated,
  }[state.colSort];
  const cols = state.collections.filter((c) => !q || c.name.toLowerCase().includes(q)).sort(by);
  const nodes = cols.map((col) => {
    const html = colCardHtml(col, byId);
    let cached = colCache.get(col.id);
    if (!cached || cached.html !== html) {
      cached = { html, el: elementFrom(html) };
      colCache.set(col.id, cached);
    }
    return cached.el;
  });
  createColEl ||= elementFrom(`
    <article class="card add-card" data-col="create">
      <div class="thumb"><div class="plus">${ICONS.plus}</div><span>Create collection</span></div>
      <div class="info">
        <div class="avatar"></div>
        <div class="text"><h3 class="title">Create collection</h3><div class="channel">Group videos from any channel</div></div>
      </div>
    </article>`);
  if (!q) nodes.push(createColEl);
  if (q && !cols.length) nodes.push(elementFrom(`<p class="empty">No collections match “${esc(state.query)}”.</p>`));
  placeNodes(colGrid, nodes);
  $("#count").textContent = plural(state.collections.length, "collection");
}

function renderCollectionHeader() {
  const col = state.view === "collection" && currentCollection();
  $("#colHeader").hidden = !col;
  if (!col) return;
  const byId = itemsById();
  const vids = col.videos.map((id) => byId.get(id)).filter(Boolean);
  const total = vids.reduce((sum, v) => sum + (v.duration || 0), 0);
  $("#colTitle").textContent = col.name;
  $("#colMeta").textContent = [plural(vids.length, "video"), total ? `${fmtDuration(total)} total` : "",
    `updated ${timeAgo(col.updated)}`].filter(Boolean).join(" • ");
  $("#colHeader [data-col='play']").disabled = !vids.some((v) => v.exists);
  $("#colHeader [data-col='removeCover']").hidden = !col.cover;
  $("#colHeader [data-col='cover']").textContent = col.cover ? "Change cover" : "Add cover";
}

// Switches between Home, the Collections list and one collection.
function go(view, collectionId = null) {
  Object.assign(state, {
    view, collectionId, selected: null, selecting: false, picked: new Set(),
    channel: null, missingOnly: false, query: "",
  });
  $("#search").value = "";
  hidePopups();
  quietCards = true;
  try { render(); } finally { quietCards = false; }
  scrollTo({ top: 0 });
  enterView();
}

const colActions = {
  open(col) { go("collection", col.id); },
  back() { go("collections"); },

  async create() { await actions.createCollection(); },

  async play(col) {
    const r = await api("/api/collection/play", { cid: col.id });
    toast(`Playing ${plural(r.count, "video")} in your player`);
  },

  cover(col) {
    const input = $("#colCoverInput");
    input.value = "";
    input.onchange = () => setCollectionCover(col.id, input.files[0]).catch((e) => toast(e.message));
    input.click();
  },

  async removeCover(col) {
    await api("/api/collection/remove-cover", { cid: col.id });
    toast("Cover removed");
  },

  async rename(col) {
    const name = await dialog({ title: "Rename collection", text: "", value: col.name, ok: "Save" });
    if (name && name.trim() && name.trim() !== col.name) await api("/api/collection/rename", { cid: col.id, name });
  },

  async delete(col) {
    const ok = await dialog({
      title: "Delete collection?",
      text: `“${col.name}” will be deleted. Its videos stay in your library.`,
      ok: "Delete", danger: true,
    });
    if (!ok) return;
    await api("/api/collection/delete", { cid: col.id });
    if (state.view === "collection") go("collections");
    toast("Collection deleted");
  },
};

async function setCollectionCover(cid, file) {
  if (!file || !file.type.startsWith("image/")) return toast("That isn't an image");
  await api("/api/collection/cover", { cid, image: await coverDataUrl(file) });
  toast("Cover updated");
}

async function runCol(name, cid) {
  hidePopups();
  try {
    const col = state.collections.find((c) => c.id === cid);
    if (col || name === "create" || name === "back") await colActions[name](col);
  } catch (e) {
    toast(e.message);
  }
}

// "Save to collection": tick the collections the videos belong in, or make a new one.
function saveDialog(items) {
  const modal = $("#saveModal"), list = $("#saveList"), form = $("#saveForm"), nameInput = $("#saveNewName");
  const ids = items.map((i) => i.id);
  $("#saveTitle").textContent = items.length > 1 ? `Save ${items.length} videos to…` : "Save to…";

  const draw = () => {
    const cols = [...state.collections].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
    list.innerHTML = cols.length
      ? cols.map((c) => {
        const inside = ids.filter((id) => c.videos.includes(id)).length;
        return `<label class="save-row">
          <input type="checkbox" data-cid="${c.id}" ${inside === ids.length ? "checked" : ""} ${inside && inside < ids.length ? "data-some" : ""}>
          <span class="save-name">${esc(c.name)}</span><span class="save-count">${plural(c.videos.length, "video")}</span>
        </label>`;
      }).join("")
      : `<p class="save-empty">No collections yet. Create the first one below.</p>`;
    list.querySelectorAll("[data-some]").forEach((b) => { b.indeterminate = true; });
  };

  draw();
  nameInput.value = "";
  modal.hidden = false;
  (list.querySelector("input") || nameInput).focus();

  return new Promise((resolve) => {
    const close = () => {
      modal.hidden = true;
      list.onchange = form.onsubmit = modal.onmousedown = modal.onkeydown = $("#saveDone").onclick = null;
      resolve();
    };
    list.onchange = async (e) => {
      const box = e.target, col = state.collections.find((c) => c.id === box.dataset.cid);
      try {
        await api(`/api/collection/${box.checked ? "add" : "remove"}`, { cid: col.id, ids });
        toast(box.checked ? `Saved to “${col.name}”` : `Removed from “${col.name}”`);
      } catch (err) { toast(err.message); }
      draw();
    };
    form.onsubmit = async (e) => {
      e.preventDefault();
      const name = nameInput.value.trim();
      if (!name) return nameInput.focus();
      try {
        await api("/api/collection/create", { name, ids });
        toast(`Saved to “${name}”`);
        nameInput.value = "";
      } catch (err) { toast(err.message); }
      draw();
    };
    $("#saveDone").onclick = close;
    modal.onmousedown = (e) => { if (e.target === modal) close(); };
    modal.onkeydown = (e) => { if (e.key === "Escape") close(); };
  });
}

// Every channel in the library, named or taken from the folder.
function allChannels() {
  const map = new Map();
  for (const i of state.items) {
    const c = map.get(i.channel) || { name: i.channel, avatar: avatarUrl(i), count: 0 };
    c.count++;
    map.set(i.channel, c);
  }
  return [...map.values()].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
}

function renderChannelToggle() {
  const toggle = $("#channelToggle");
  const c = state.channel && state.items.find((i) => i.channel === state.channel);
  toggle.classList.toggle("active", !!c);
  toggle.title = c ? `Showing only ${c.channel}` : "Show one channel";
  toggle.innerHTML = c
    ? `<span class="avatar" style="background:${avatarColor(c.channel)}">${avatarInner(c.channel, avatarUrl(c))}</span>
       ${esc(c.channel)}<span class="dd-clear" data-clear title="Show all channels">×</span>`
    : `${ICONS.channels}Channels${ICONS.chevron}`;
}

function openChannelMenu() {
  const menu = $("#channelMenu"), toggle = $("#channelToggle");
  const channels = allChannels();
  const item = (name, icon, label, count) => `
    <button class="dd-item ${state.channel === name ? "active" : ""}" data-channel="${esc(name ?? "")}">
      ${icon}<span class="dd-name">${esc(label)}</span>${count == null ? "" : `<span class="dd-count">${count}</span>`}
    </button>`;
  menu.innerHTML = `
    <h4>Channels</h4>
    ${item(null, `<span class="all-icon">${ICONS.channels}</span>`, "All videos", state.items.length)}
    <hr>
    ${channels.map((c) => item(c.name,
      `<span class="avatar" style="background:${avatarColor(c.name)}">${avatarInner(c.name, c.avatar)}</span>`,
      c.name, c.count)).join("")}
    ${channels.length ? "" : `<p class="dd-empty">No videos yet. Add some and their channels show up here.</p>`}`;
  hidePopups();
  menu.hidden = false;
  toggle.setAttribute("aria-expanded", "true");
  const r = toggle.getBoundingClientRect();
  menu.style.left = `${Math.max(8, Math.min(r.left, innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${r.bottom + 6}px`;
}

function filterChannel(name) {
  hidePopups();
  state.channel = name || null;
  render();
  scrollTo({ top: 0 });
}

function updateMenu() {
  const item = selectedItem();
  document.querySelectorAll("[data-needs]").forEach((b) => {
    const needs = b.dataset.needs;
    b.disabled = needs === "picked" ? !state.picked.size : needs === "cover" ? !item?.cover : !item;
  });
  const n = state.picked.size;
  $("#menu").classList.toggle("selecting", state.selecting);
  grid.classList.toggle("selecting", state.selecting);
  $("#selCount").textContent = `${n} selected`;
  $("[data-action='bulkRemoveCover']").disabled = !pickedItems().some((i) => i.cover);
  // One button: marks all as watched, or unwatched when they all already are.
  const allWatched = n > 0 && pickedItems().every((i) => i.watched);
  $("[data-action='bulkWatched'] span").textContent = allWatched ? "Mark as unwatched" : "Mark as watched";
}

const pickedItems = () => state.items.filter((i) => state.picked.has(i.id));

// ---------------------------------------------------------------- selection mode

function setSelecting(on, initial = []) {
  state.selecting = on;
  state.picked = new Set(on ? initial : []);
  state.lastPicked = initial[initial.length - 1] ?? null;
  if (on) state.selected = null;
  hidePopups();
  render();
}

// Click toggles one video; Shift + click picks everything between it and the last one.
function togglePick(id, range) {
  const order = visibleItems().map((i) => i.id);
  if (range && state.lastPicked && order.includes(state.lastPicked)) {
    const [a, b] = [order.indexOf(state.lastPicked), order.indexOf(id)].sort((x, y) => x - y);
    order.slice(a, b + 1).forEach((i) => state.picked.add(i));
  } else if (state.picked.has(id)) {
    state.picked.delete(id);
  } else {
    state.picked.add(id);
  }
  state.lastPicked = id;
  render();
}

const selectedItem = () => state.items.find((i) => i.id === state.selected);

function select(id) {
  state.selected = id;
  grid.querySelectorAll(".card[data-id]").forEach((c) => c.classList.toggle("selected", c.dataset.id === id));
  updateMenu();
}

// ---------------------------------------------------------------- actions

const actions = {
  async add() { await addVideos("files"); },
  async addFolder() { await addVideos("folder"); },

  async play(item) {
    try {
      await api("/api/open", { id: item.id });
      toast(`Opening “${item.title}”`);
    } catch (e) { toast(e.message); }
  },

  cover(item) {
    state.coverTarget = item.id;
    $("#coverInput").value = "";
    $("#coverInput").click();
  },

  async removeCover(item) {
    thumbTried.delete(item.id); // grab a fresh frame
    await api("/api/remove-cover", { id: item.id });
    toast("Cover removed");
  },

  async rename(item) {
    const title = await dialog({ title: "Rename", text: "", value: item.title, ok: "Save" });
    if (title && title.trim() && title !== item.title) await api("/api/rename", { id: item.id, title });
  },

  async channel(item) {
    const result = await channelDialog([item]);
    if (result) {
      await api("/api/channel", { id: item.id, ...result });
      toast("Channel updated");
    }
  },

  async reveal(item) { await api("/api/reveal", { id: item.id }); },

  async toggleWatched(item) {
    await api("/api/watched", { id: item.id, watched: !item.watched });
    toast(item.watched ? "Marked as unwatched" : "Marked as watched");
  },

  async collect(item) { await saveDialog([item]); },

  async uncollect(item) {
    const col = currentCollection();
    if (!col) return;
    await api("/api/collection/remove", { cid: col.id, ids: [item.id] });
    toast(`Removed from “${col.name}”`);
  },

  async locate(item) {
    toast(`Find “${item.title}” in the dialog…`, 60000);
    const res = await api("/api/locate", { id: item.id });
    if (!res.relinked) return toast("Nothing changed");
    thumbTried.delete(item.id);
    toast("File found");
    // Moved a whole folder? Fix the other videos from it in one go.
    const others = res.siblings || [];
    if (!others.length) return;
    const ok = await dialog({
      title: `Found ${plural(others.length, "more missing video")}`,
      text: `${others.length === 1 ? "It is" : "They are"} in the same folder as the file you picked. Relink ${others.length === 1 ? "it" : "them"} too?`,
      ok: "Relink",
    });
    if (ok) {
      others.forEach((id) => thumbTried.delete(id));
      const r = await api("/api/relink-folder", { ids: others, folder: res.folder });
      toast(`Relinked ${plural(r.relinked + 1, "video")}`);
    }
  },

  async delete(item) {
    const ok = await dialog({
      title: "Delete entry?",
      text: `“${item.title}” will be removed from the library. The video file on your drive is not touched.`,
      ok: "Delete", danger: true,
    });
    if (ok) {
      await api("/api/delete", { id: item.id });
      toast("Entry deleted");
    }
  },

  // ---- selection mode (these don't take a single video)

  async createCollection() {
    const name = await dialog({ title: "New collection", text: "", value: "", ok: "Create" });
    if (!name || !name.trim()) return;
    await api("/api/collection/create", { name });
    toast(`Created “${name.trim()}”`);
  },

  async bulkWatched() {
    const items = pickedItems();
    if (!items.length) return;
    const watched = !items.every((i) => i.watched);
    await api("/api/watched", { ids: items.map((i) => i.id), watched });
    toast(`Marked ${plural(items.length, "video")} as ${watched ? "watched" : "unwatched"}`);
  },

  async bulkCollect() {
    const items = pickedItems();
    if (items.length) await saveDialog(items);
  },

  async bulkUncollect() {
    const col = currentCollection(), items = pickedItems();
    if (!col || !items.length) return;
    await api("/api/collection/remove", { cid: col.id, ids: items.map((i) => i.id) });
    toast(`Removed ${plural(items.length, "video")} from “${col.name}”`);
  },

  selectMode() { setSelecting(true, state.selected ? [state.selected] : []); },
  exitSelect() { setSelecting(false); },
  selectAll() {
    visibleItems().forEach((i) => state.picked.add(i.id));
    render();
  },
  selectNone() {
    state.picked.clear();
    render();
  },

  async bulkChannel() {
    const items = pickedItems();
    const result = items.length && await channelDialog(items);
    if (result) {
      await api("/api/channel", { ids: items.map((i) => i.id), ...result });
      toast(`Moved ${plural(items.length, "video")} to “${result.name}”`);
    }
  },

  async bulkRemoveCover() {
    const items = pickedItems().filter((i) => i.cover);
    if (!items.length) return;
    items.forEach((i) => thumbTried.delete(i.id)); // grab fresh frames
    await api("/api/remove-cover", { ids: items.map((i) => i.id) });
    toast(`Removed ${plural(items.length, "cover")}`);
  },

  async bulkDelete() {
    const items = pickedItems();
    if (!items.length) return;
    const ok = await dialog({
      title: `Delete ${plural(items.length, "entry", "entries")}?`,
      text: `${plural(items.length, "video")} will be removed from the library. The video files on your drive are not touched.`,
      ok: "Delete", danger: true,
    });
    if (ok) {
      await api("/api/delete", { ids: items.map((i) => i.id) });
      toast(`Deleted ${plural(items.length, "entry", "entries")}`);
      setSelecting(false);
    }
  },
};

const GLOBAL_ACTIONS = new Set(["add", "addFolder", "selectMode", "exitSelect", "selectAll", "selectNone",
  "bulkChannel", "bulkRemoveCover", "bulkDelete", "createCollection", "bulkCollect", "bulkUncollect", "bulkWatched"]);

const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

async function run(name, item) {
  hidePopups();
  try {
    if (GLOBAL_ACTIONS.has(name)) return await actions[name]();
    item = item || selectedItem();
    if (item) await actions[name](item);
  } catch (e) {
    toast(e.message);
  }
}

async function addVideos(mode) {
  toast(mode === "folder" ? "Choose a folder in the dialog…" : "Choose videos in the dialog…", 60000);
  // Inside a collection, picked videos also go into it (even ones already in the library).
  const col = state.view === "collection" ? currentCollection() : null;
  const { added, picked } = await api("/api/add", { mode, collection: col?.id });
  if (col) toast(picked ? `Added ${plural(picked, "video")} to “${col.name}”` : "Nothing added");
  else toast(added.length ? `Added ${plural(added.length, "video")}` : "Nothing new added");
  if (added.length) select(added[added.length - 1]);
}

const readDataUrl = (file) => new Promise((resolve, reject) => {
  const r = new FileReader();
  r.onload = () => resolve(r.result);
  r.onerror = reject;
  r.readAsDataURL(file);
});

async function setCoverFromFile(id, file) {
  if (!file || !file.type.startsWith("image/")) return toast("That isn't an image");
  await api("/api/cover", { id, image: await coverDataUrl(file) });
  toast("Cover updated");
}

// Covers never show wider than ~1280px: bigger or heavy images are scaled down to a
// JPEG before saving (GIFs are kept as they are, so they stay animated).
async function coverDataUrl(file, maxWidth = 1280) {
  const url = await readDataUrl(file);
  if (file.type === "image/gif") return url;
  const img = new Image();
  img.src = url;
  await img.decode();
  if (img.naturalWidth <= maxWidth && file.size <= 400 * 1024) return url;
  const c = document.createElement("canvas");
  c.width = Math.min(maxWidth, img.naturalWidth);
  c.height = Math.round((c.width * img.naturalHeight) / img.naturalWidth);
  c.getContext("2d").drawImage(img, 0, 0, c.width, c.height);
  return c.toDataURL("image/jpeg", 0.9);
}

// Center-crops a photo to a small square, like a channel avatar.
async function squarePhoto(file, size = 256) {
  const img = new Image();
  img.src = await readDataUrl(file);
  await img.decode();
  const side = Math.min(img.naturalWidth, img.naturalHeight);
  const c = document.createElement("canvas");
  c.width = c.height = size;
  c.getContext("2d").drawImage(img, (img.naturalWidth - side) / 2, (img.naturalHeight - side) / 2, side, side, 0, 0, size, size);
  return c.toDataURL("image/jpeg", 0.92); // ~10x smaller than PNG for photos
}

// ---------------------------------------------------------------- auto thumbnails
// Grabs a frame from each video without a cover. Works for whatever the browser
// can decode (mp4/webm/most mkv); others keep the placeholder until you add a cover.
// Auto covers from older versions (640px) are re-captured once at full size.

const THUMB_WIDTH = 1280;
const thumbQueue = [];
let thumbBusy = false;

const thumbTried = new Set(); // one attempt per video per session, whatever the server says
const needsThumb = (i) => i.exists && !thumbTried.has(i.id) && (i.cover ? i.coverAuto && !i.thumbHQ : !i.autoTried);

function queueAutoThumbs() {
  for (const i of state.items) {
    if (needsThumb(i) && !thumbQueue.includes(i.id)) thumbQueue.push(i.id);
  }
  pumpThumbs();
}

async function pumpThumbs() {
  if (thumbBusy) return;
  thumbBusy = true;
  while (thumbQueue.length) {
    const id = thumbQueue.shift();
    const item = state.items.find((i) => i.id === id);
    if (!item || !needsThumb(item)) continue;
    thumbTried.add(id);
    const result = await captureFrame(id);
    try { await api("/api/auto-thumb", { id, ...result }); } catch { /* entry was deleted meanwhile */ }
  }
  thumbBusy = false;
}

function captureFrame(id) {
  return new Promise((resolve) => {
    const v = document.createElement("video");
    let done = false;
    const finish = (image = null) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      const duration = isFinite(v.duration) ? v.duration : null;
      v.removeAttribute("src");
      v.load();
      resolve({ image, duration });
    };
    const timer = setTimeout(() => finish(), 20000);
    v.muted = true;
    v.preload = "auto";
    v.addEventListener("error", () => finish());
    v.addEventListener("loadedmetadata", () => {
      const d = v.duration;
      v.currentTime = isFinite(d) && d > 0 ? Math.min(d * 0.2, 300) : 1;
    });
    v.addEventListener("seeked", () => {
      if (!v.videoWidth) return finish();
      const c = document.createElement("canvas");
      c.width = Math.min(THUMB_WIDTH, v.videoWidth);
      c.height = Math.round((c.width * v.videoHeight) / v.videoWidth);
      c.getContext("2d").drawImage(v, 0, 0, c.width, c.height);
      finish(c.toDataURL("image/jpeg", 0.85));
    });
    v.src = `/media/${id}`;
  });
}

// ---------------------------------------------------------------- ui helpers

let toastTimer;
function toast(msg, ms = 2600) {
  const t = $("#toast");
  t.textContent = msg;
  t.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove("show"), ms);
}

function dialog({ title, text, value, ok = "OK", danger = false }) {
  const modal = $("#modal"), input = $("#modalInput"), okBtn = $("#modalOk");
  $("#modalTitle").textContent = title;
  $("#modalText").textContent = text;
  $("#modalText").hidden = !text;
  input.hidden = value === undefined;
  input.value = value ?? "";
  okBtn.textContent = ok;
  okBtn.classList.toggle("danger", danger);
  modal.hidden = false;
  (value === undefined ? okBtn : input).focus();
  if (value !== undefined) input.select();

  return new Promise((resolve) => {
    const close = (result) => {
      modal.hidden = true;
      $("#modalForm").onsubmit = $("#modalCancel").onclick = modal.onmousedown = modal.onkeydown = null;
      resolve(result);
    };
    $("#modalForm").onsubmit = (e) => { e.preventDefault(); close(value === undefined ? true : input.value); };
    $("#modalCancel").onclick = () => close(null);
    modal.onmousedown = (e) => { if (e.target === modal) close(null); };
    modal.onkeydown = (e) => { if (e.key === "Escape") close(null); };
  });
}

// Edits the channel of one or several videos.
// Resolves to { name, photo, applyToAll } or null if cancelled.
// photo: null = keep, "remove", or a data URL.
function channelDialog(items) {
  const modal = $("#channelModal"), form = $("#channelForm"), nameInput = $("#chName");
  const preview = $("#chAvatar"), allBox = $("#chAll"), fileInput = $("#avatarInput");
  const channels = new Map();
  for (const i of state.items) if (!channels.has(i.channel) || i.channelAvatar) channels.set(i.channel, avatarUrl(i));
  const first = items[0];
  const common = items.every((i) => i.channel === first.channel) ? first.channel : "";
  const others = items.length > 1 ? 0 : state.items.filter((i) => i.id !== first.id && i.channel === first.channel).length;
  let photo = null;

  $("#chTitle").textContent = items.length > 1 ? `Edit channel of ${items.length} videos` : "Edit channel";
  $("#channelList").innerHTML = [...channels.keys()].sort().map((c) => `<option value="${esc(c)}">`).join("");
  nameInput.value = common;
  allBox.checked = false;
  $("#chAllRow").hidden = !others;
  $("#chAllText").textContent = `Also apply to the other ${plural(others, "video")} from “${first.channel}”`;

  const refresh = () => {
    const name = nameInput.value.trim() || common || "?";
    const existing = channels.get(name) || (allBox.checked ? channels.get(first.channel) : null);
    const src = photo === "remove" ? null : photo || existing;
    preview.style.background = avatarColor(name);
    preview.innerHTML = avatarInner(name, src);
    $("#chPhotoRemove").hidden = !src;
  };
  const usePhoto = async (file) => {
    if (!file || !file.type.startsWith("image/")) return toast("That isn't an image");
    photo = await squarePhoto(file);
    refresh();
  };

  refresh();
  modal.hidden = false;
  nameInput.focus();
  nameInput.select();

  return new Promise((resolve) => {
    const close = (result) => {
      modal.hidden = true;
      form.onsubmit = modal.onmousedown = modal.onkeydown = nameInput.oninput = allBox.onchange = null;
      $("#chCancel").onclick = $("#chPhoto").onclick = $("#chPhotoRemove").onclick = fileInput.onchange = null;
      preview.ondragover = preview.ondragleave = preview.ondrop = null;
      resolve(result);
    };
    form.onsubmit = (e) => {
      e.preventDefault();
      const name = nameInput.value.trim();
      if (!name) return nameInput.focus();
      close({ name, photo, applyToAll: allBox.checked });
    };
    nameInput.oninput = allBox.onchange = refresh;
    $("#chCancel").onclick = () => close(null);
    $("#chPhoto").onclick = () => { fileInput.value = ""; fileInput.click(); };
    $("#chPhotoRemove").onclick = () => { photo = "remove"; refresh(); };
    fileInput.onchange = () => usePhoto(fileInput.files[0]);
    preview.ondragover = (e) => { e.preventDefault(); preview.classList.add("drop"); };
    preview.ondragleave = () => preview.classList.remove("drop");
    preview.ondrop = (e) => {
      e.preventDefault();
      preview.classList.remove("drop");
      usePhoto([...e.dataTransfer.files].find((f) => f.type.startsWith("image/")));
    };
    modal.onmousedown = (e) => { if (e.target === modal) close(null); };
    modal.onkeydown = (e) => { if (e.key === "Escape") close(null); };
  });
}

function showCtx(item, x, y) {
  select(item.id);
  ctx.querySelector('[data-needs="cover"]').disabled = !item.cover;
  ctx.querySelector('[data-action="toggleWatched"]').textContent = item.watched ? "Mark as unwatched" : "Mark as watched";
  ctx.hidden = false;
  const { width, height } = ctx.getBoundingClientRect();
  ctx.style.left = `${Math.min(x, innerWidth - width - 8)}px`;
  ctx.style.top = `${Math.min(y, innerHeight - height - 8)}px`;
}
const hidePopups = () => {
  ctx.hidden = true;
  colCtx.hidden = true;
  $("#themeMenu").hidden = true;
  $("#channelMenu").hidden = true;
  $("#channelToggle").setAttribute("aria-expanded", "false");
};

// ---------------------------------------------------------------- events

const cardItem = (el) => state.items.find((i) => i.id === el?.closest(".card")?.dataset.id);

grid.addEventListener("click", (e) => {
  const card = e.target.closest(".card");
  if (state.selecting) {
    if (card?.dataset.id) togglePick(card.dataset.id, e.shiftKey);
    return;
  }
  if (!card) return select(null);
  if (card.classList.contains("add-card")) return run("add");
  const btn = e.target.closest("[data-action]");
  const item = cardItem(card);
  // Ctrl + click starts picking several videos, like in File Explorer.
  if (e.ctrlKey || e.metaKey) {
    return setSelecting(true, [...new Set([state.selected, item.id].filter(Boolean))]);
  }
  if (btn?.dataset.action === "play") return run("play", item);
  if (btn?.dataset.action === "locate") {
    select(item.id);
    return run("locate", item);
  }
  if (btn?.dataset.action === "channel") {
    select(item.id);
    return run("channel", item);
  }
  if (btn?.dataset.action === "menu") {
    const r = btn.getBoundingClientRect();
    return showCtx(item, r.right - 200, r.bottom + 4);
  }
  select(item.id);
});

grid.addEventListener("dblclick", (e) => {
  const item = cardItem(e.target);
  if (item && !state.selecting && !e.target.closest("[data-action]")) run("play", item);
});

grid.addEventListener("contextmenu", (e) => {
  const item = cardItem(e.target);
  if (!item) return;
  e.preventDefault();
  if (!state.selecting) showCtx(item, e.clientX, e.clientY);
});

// Shift + click would otherwise highlight text across cards.
grid.addEventListener("mousedown", (e) => { if (state.selecting && e.shiftKey) e.preventDefault(); });

$("#menu").addEventListener("click", (e) => {
  const tab = e.target.closest("button[data-view]");
  if (tab) return go(tab.dataset.view);
  const btn = e.target.closest("button[data-action]");
  if (btn) run(btn.dataset.action);
});

$("#brand").addEventListener("click", () => go("home"));

// Collections list: open a collection, its ⋮ menu, or create one.
let ctxCollection = null;
function showColCtx(cid, x, y) {
  hidePopups();
  ctxCollection = cid;
  const col = state.collections.find((c) => c.id === cid);
  colCtx.querySelector("[data-col='removeCover']").hidden = !col?.cover;
  colCtx.querySelector("[data-col='cover']").textContent = col?.cover ? "Change cover" : "Add cover";
  colCtx.hidden = false;
  const { width, height } = colCtx.getBoundingClientRect();
  colCtx.style.left = `${Math.min(x, innerWidth - width - 8)}px`;
  colCtx.style.top = `${Math.min(y, innerHeight - height - 8)}px`;
}

colGrid.addEventListener("click", (e) => {
  if (e.target.closest("[data-col='create']")) return runCol("create");
  const card = e.target.closest("[data-cid]");
  if (!card) return;
  const more = e.target.closest("[data-colmenu]");
  if (more) {
    const r = more.getBoundingClientRect();
    return showColCtx(card.dataset.cid, r.right - 200, r.bottom + 4);
  }
  runCol("open", card.dataset.cid);
});

// Drop an image onto a collection to make it the cover.
colGrid.addEventListener("dragover", (e) => {
  const card = e.target.closest("[data-cid]");
  e.preventDefault();
  e.dataTransfer.dropEffect = card ? "copy" : "none";
  colGrid.querySelectorAll(".card.drop").forEach((c) => c !== card && c.classList.remove("drop"));
  card?.classList.add("drop");
});
colGrid.addEventListener("dragleave", (e) => {
  if (!colGrid.contains(e.relatedTarget)) colGrid.querySelectorAll(".card.drop").forEach((c) => c.classList.remove("drop"));
});
colGrid.addEventListener("drop", (e) => {
  e.preventDefault();
  colGrid.querySelectorAll(".card.drop").forEach((c) => c.classList.remove("drop"));
  const card = e.target.closest("[data-cid]");
  const file = [...e.dataTransfer.files].find((f) => f.type.startsWith("image/"));
  if (card && file) setCollectionCover(card.dataset.cid, file).catch((err) => toast(err.message));
  else if (card) toast("Drop an image file to use it as the cover");
});

colGrid.addEventListener("contextmenu", (e) => {
  const card = e.target.closest("[data-cid]");
  if (!card) return;
  e.preventDefault();
  showColCtx(card.dataset.cid, e.clientX, e.clientY);
});

colCtx.addEventListener("click", (e) => {
  const btn = e.target.closest("[data-col]");
  if (btn) runCol(btn.dataset.col, ctxCollection);
});

$("#colHeader").addEventListener("click", (e) => {
  const btn = e.target.closest("[data-col]");
  if (btn) runCol(btn.dataset.col, state.collectionId);
});

// Themes: purely cosmetic, remembered on this computer. LIQUID and STYMU come
// from your design systems; their colours live in style.css.
const THEMES = [
  { id: "classic-dark", group: "Classic", name: "Dark", swatch: ["#0e0f12", "#25272d", "#ff6a2b"] },
  { id: "classic-light", group: "Classic", name: "Light", swatch: ["#fbfbfc", "#f0f1f3", "#f05a1a"] },
  { id: "liquid-sky", group: "LIQUID", name: "Sky", swatch: ["#f2fbff", "#b6e0fb", "#0a84ff"] },
  { id: "liquid-glass", group: "LIQUID", name: "Glass", swatch: ["#06161f", "#123043", "#3ad4f2"] },
  { id: "stymu-sleeve", group: "STYMU", name: "Sleeve", swatch: ["#ece0c8", "#d8901f", "#a8431a"] },
  { id: "stymu-cabinet", group: "STYMU", name: "Cabinet", swatch: ["#0a0a09", "#c8e04a", "#e2773d"] },
];
const themeMenu = $("#themeMenu");
const currentTheme = () => document.documentElement.dataset.theme || "classic-dark";

// Saved in the app folder (data/settings.json), so it travels with it.
const saveSetting = (key, value) =>
  fetch("/api/settings", { method: "POST", body: JSON.stringify({ [key]: value }) }).catch(() => {});

function setTheme(id, save = true) {
  if (!THEMES.some((t) => t.id === id)) id = "classic-dark";
  document.documentElement.dataset.theme = id;
  if (save) saveSetting("theme", id);
}

// Each theme's swatch is the app's logo in that theme's colours: its page colour behind,
// one tile filled with the accent and three tiles in the accent and surface colours.
function swatchSvg([page, surface, accent]) {
  return `<svg class="swatch" viewBox="0 0 64 64" aria-hidden="true">
    <rect x="1" y="1" width="62" height="62" rx="14" fill="${page}" stroke="#8886" stroke-width="2"/>
    <rect x="10" y="16" width="20" height="14" rx="3.5" fill="${accent}"/>
    <g fill="${surface}" stroke="${accent}" stroke-width="3.5">
      <rect x="35.75" y="17.75" width="16.5" height="10.5" rx="2"/>
      <rect x="11.75" y="35.75" width="16.5" height="10.5" rx="2"/>
      <rect x="35.75" y="35.75" width="16.5" height="10.5" rx="2"/>
    </g>
  </svg>`;
}

function openThemeMenu() {
  let html = "<h4>Theme</h4>", group = null;
  for (const t of THEMES) {
    if (t.group !== group) html += `<h5>${esc(group = t.group)}</h5>`;
    html += `<button class="dd-item ${t.id === currentTheme() ? "active" : ""}" data-theme-id="${t.id}">
      ${swatchSvg(t.swatch)}
      <span class="dd-name">${esc(t.name)}</span>${t.id === currentTheme() ? ICONS.check.replace("<svg", '<svg class="dd-check"') : ""}
    </button>`;
  }
  hidePopups();
  themeMenu.innerHTML = html;
  themeMenu.hidden = false;
  const r = $("#themeBtn").getBoundingClientRect();
  themeMenu.style.left = `${Math.max(8, Math.min(r.right - themeMenu.offsetWidth, innerWidth - themeMenu.offsetWidth - 8))}px`;
  themeMenu.style.top = `${r.bottom + 8}px`;
}

$("#themeBtn").addEventListener("click", () => (themeMenu.hidden ? openThemeMenu() : hidePopups()));
themeMenu.addEventListener("click", (e) => {
  const item = e.target.closest("[data-theme-id]");
  if (!item) return;
  setTheme(item.dataset.themeId);
  openThemeMenu(); // stays open so you can flip through themes
});
setTheme(currentTheme(), false); // the server already painted the saved theme

ctx.addEventListener("click", (e) => {
  const btn = e.target.closest("button[data-action]");
  if (btn) run(btn.dataset.action);
});

$("#channelToggle").addEventListener("click", (e) => {
  if (e.target.closest("[data-clear]")) return filterChannel(null);
  if ($("#channelMenu").hidden) openChannelMenu();
  else hidePopups();
});

$("#channelMenu").addEventListener("click", (e) => {
  const btn = e.target.closest("[data-channel]");
  if (btn) filterChannel(btn.dataset.channel);
});

document.addEventListener("mousedown", (e) => {
  if (!e.target.closest("#ctx, #colCtx, #channelMenu, #channelToggle, #themeMenu, #themeBtn")) hidePopups();
});
addEventListener("blur", hidePopups);
addEventListener("scroll", (e) => { if (!$("#channelMenu").contains(e.target)) hidePopups(); }, true);

// Thumbnail size: slider or Ctrl + mouse wheel; remembered on this computer.
const sizeSlider = $("#sizeSlider");

let saveSizeTimer;
function setThumbSize(px, save = true) {
  px = Math.max(+sizeSlider.min, Math.min(+sizeSlider.max, Math.round(px / 10) * 10));
  sizeSlider.value = px;
  for (const g of [grid, colGrid]) {
    g.style.setProperty("--card-min", `${px}px`);
    g.dataset.size = px < 240 ? "s" : px > 420 ? "l" : "m";
  }
  if (save) {
    clearTimeout(saveSizeTimer); // the slider fires on every step; save once it settles
    saveSizeTimer = setTimeout(() => saveSetting("thumbSize", px), 400);
  }
}

sizeSlider.addEventListener("input", () => setThumbSize(+sizeSlider.value));
for (const g of [grid, colGrid]) {
  g.addEventListener("wheel", (e) => {
    if (!e.ctrlKey) return;
    e.preventDefault();
    setThumbSize(+sizeSlider.value - Math.sign(e.deltaY) * 20);
  }, { passive: false });
}

setThumbSize(300, false); // the saved size arrives with the first /api/state

$("#chips").addEventListener("click", (e) => {
  const chip = e.target.closest(".chip");
  if (!chip) return;
  if (chip.id === "missingChip") {
    state.missingOnly = !state.missingOnly;
    render();
    return scrollTo({ top: 0 });
  }
  if (chip.dataset.colsort) {
    document.querySelectorAll(".chip[data-colsort]").forEach((c) => c.classList.toggle("active", c === chip));
    state.colSort = chip.dataset.colsort;
    return render();
  }
  document.querySelectorAll(".chip[data-sort]").forEach((c) => c.classList.toggle("active", c === chip));
  state.sort = chip.dataset.sort;
  render();
});

$("#search").addEventListener("input", (e) => { state.query = e.target.value.trim(); render(); });
$("#searchForm").addEventListener("submit", (e) => e.preventDefault());

$("#coverInput").addEventListener("change", (e) => {
  const file = e.target.files[0];
  if (file && state.coverTarget) setCoverFromFile(state.coverTarget, file).catch((err) => toast(err.message));
});

// Drop an image onto a card to make it the cover.
grid.addEventListener("dragover", (e) => {
  const card = e.target.closest(".card[data-id]");
  e.preventDefault();
  e.dataTransfer.dropEffect = card ? "copy" : "none";
  grid.querySelectorAll(".card.drop").forEach((c) => c !== card && c.classList.remove("drop"));
  card?.classList.add("drop");
});
grid.addEventListener("dragleave", (e) => {
  if (!grid.contains(e.relatedTarget)) grid.querySelectorAll(".card.drop").forEach((c) => c.classList.remove("drop"));
});
grid.addEventListener("drop", (e) => {
  e.preventDefault();
  grid.querySelectorAll(".card.drop").forEach((c) => c.classList.remove("drop"));
  const item = cardItem(e.target);
  const file = [...e.dataTransfer.files].find((f) => f.type.startsWith("image/"));
  if (item && file) {
    select(item.id);
    setCoverFromFile(item.id, file).catch((err) => toast(err.message));
  } else if (item) {
    toast("Drop an image file to use it as the cover");
  }
});
// Forms are handled in JS; never let one reload the page.
document.addEventListener("submit", (e) => e.preventDefault());
document.addEventListener("dragover", (e) => e.preventDefault());
document.addEventListener("drop", (e) => e.preventDefault());

// Paste an image (Ctrl+V) to set the selected video's cover.
document.addEventListener("paste", (e) => {
  if (!state.selected || e.target.matches("input") || document.querySelector(".modal-backdrop:not([hidden])")) return;
  const file = [...e.clipboardData.files].find((f) => f.type.startsWith("image/"));
  if (file) setCoverFromFile(state.selected, file).catch((err) => toast(err.message));
});

document.addEventListener("keydown", (e) => {
  if (e.target.matches?.("input") || document.querySelector(".modal-backdrop:not([hidden])")) return;
  if (state.view === "collections") {
    if (e.key === "Escape") hidePopups();
    return;
  }
  if (state.selecting) {
    if (e.key === "Escape") run("exitSelect");
    if (e.key === "Delete") run("bulkDelete");
    if (e.key.toLowerCase() === "a" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); run("selectAll"); }
    return;
  }
  if (e.key.toLowerCase() === "a" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); setSelecting(true); run("selectAll"); return; }
  if (e.key === "Escape") { hidePopups(); select(null); }
  if (!state.selected) return;
  if (e.key === "Enter") run("play");
  if (e.key === "Delete") run("delete");
  if (e.key === "F2") { e.preventDefault(); run("rename"); }
});

// Own title bar: the desktop app (Thumbnail.exe) has no Windows title bar, so the top bar
// takes its place, like Spotify or Discord. In a browser window none of this shows.
if (typeof window.ipc?.postMessage === "function") {
  const send = (m) => window.ipc.postMessage(m);
  const line = (d) => `<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1">${d}</svg>`;
  const WIN = {
    minimize: line('<path d="M0 5.5h10"/>'),
    maximize: line('<rect x=".5" y=".5" width="9" height="9"/>'),
    restore: line('<rect x=".5" y="2.5" width="7" height="7"/><path d="M2.5 2.5V.5h7v7h-2"/>'),
    close: line('<path d="M.5.5l9 9M9.5.5l-9 9"/>'),
  };
  document.documentElement.classList.add("native-frame");
  const topbar = $(".topbar");
  // Only the content scrolls (below the top bar and menu), so the scrollbar starts under
  // them and the window buttons sit right in the corner.
  const scroller = document.createElement("div");
  scroller.className = "scroller";
  $("#menu").after(scroller);
  scroller.append(...["#colHeader", "#chips", "#grid", "#colGrid"].map((s) => $(s)));
  window.scrollTo = (...args) => scroller.scrollTo(...args);
  const controls = document.createElement("div");
  controls.className = "window-controls";
  controls.innerHTML = `
    <button type="button" data-win="minimize" title="Minimize" aria-label="Minimize">${WIN.minimize}</button>
    <button type="button" data-win="maximize" title="Maximize" aria-label="Maximize">${WIN.maximize}</button>
    <button type="button" data-win="close" class="win-close" title="Close" aria-label="Close">${WIN.close}</button>`;
  topbar.append(controls);
  const maxBtn = controls.querySelector('[data-win="maximize"]');
  controls.addEventListener("click", (e) => {
    const b = e.target.closest("button[data-win]");
    if (b) send("window:" + b.dataset.win);
  });
  // The app tells us when the window is maximized (by us, Win+Up, snapping...).
  window.__setMaximized = (on) => {
    document.documentElement.classList.toggle("maximized", on);
    maxBtn.innerHTML = on ? WIN.restore : WIN.maximize;
    maxBtn.title = on ? "Restore" : "Maximize";
    maxBtn.setAttribute("aria-label", maxBtn.title);
  };
  // Empty parts of the top bar move the window; a double-click maximizes it.
  topbar.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || e.target.closest("button, input, a, form, .window-controls")) return;
    e.preventDefault();
    send(e.detail === 2 ? "window:maximize" : "window:drag");
  });
  // The page covers the window's top border, so the top edge and corners resize from here.
  for (const edge of ["n", "nw", "ne"]) {
    const grip = document.createElement("div");
    grip.className = `resize-grip resize-${edge}`;
    grip.addEventListener("mousedown", (e) => {
      if (e.button === 0) { e.preventDefault(); send("window:resize-" + edge); }
    });
    document.body.append(grip);
  }
  send("window:ready");
}

// Heartbeat: the server shuts itself down once the window is closed.
// When the app is updated the server restarts itself and its version changes;
// the page then reloads so both sides run the new code (waiting while you're mid-task).
let appVersion = null;
let warnedOutdated = false;
const ping = () => fetch("/api/ping", { method: "POST", body: "{}" })
  .then((r) => r.json())
  .then((r) => {
    appVersion ??= r.version;
    const busy = state.selecting || document.querySelector(".modal-backdrop:not([hidden])");
    if (r.version && r.version !== appVersion && !busy) {
      try { sessionStorage.setItem("restoreView", JSON.stringify({ view: state.view, collectionId: state.collectionId })); } catch { /* fine */ }
      return location.reload();
    }
    // The server couldn't restart itself (e.g. its new code has an error).
    if (r.outdated && !warnedOutdated) {
      warnedOutdated = true;
      toast("Thumbnail was updated. Close this window and open it again to finish.", 600000);
    }
  })
  .catch(() => {});
setInterval(ping, 5000);
ping();
addEventListener("pagehide", () => navigator.sendBeacon("/api/bye", "{}"));

api("/api/state")
  .then(({ settings = {} }) => {
    if (settings.thumbSize) setThumbSize(settings.thumbSize, false);
    // Back to where you were before an automatic reload.
    let saved = null;
    try { saved = JSON.parse(sessionStorage.getItem("restoreView")); sessionStorage.removeItem("restoreView"); } catch { /* fine */ }
    if (saved?.view === "collection" && !state.collections.some((c) => c.id === saved.collectionId)) saved.view = "collections";
    if (saved && saved.view !== "home") go(saved.view, saved.view === "collection" ? saved.collectionId : null);
  })
  .catch((e) => toast(e.message));
