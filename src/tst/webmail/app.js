const API_BASE = window.WEBMAIL_API_BASE || "/api/webmail";

const API = {
  status: `${API_BASE}/status`,
  refresh: `${API_BASE}/refresh`,
  list: `${API_BASE}/list`,
  read: (id) => `${API_BASE}/read?id=${encodeURIComponent(id)}`,
  send: `${API_BASE}/send`,
};

const state = {
  status: null,
  messages: [],
  selectedId: null,
  selectedMessage: null,
  composeOpen: false,
  queueOpen: true,
  queue: [],
  filter: "",
};

const app = document.querySelector("#app");
const toastRoot = document.querySelector("#toast-root");

function text(value, fallback = "") {
  return value === undefined || value === null ? fallback : String(value);
}

function escapeHtml(value) {
  return text(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function icon(name, size = 18) {
  return `<i data-lucide="${name}" style="width:${size}px;height:${size}px"></i>`;
}

function formatDate(value) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return text(value);
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function formatStatus(value) {
  return text(value, "unknown").replaceAll("-", " ");
}

function messageStatus(message) {
  const status = text(message?.status, message?.unread ? "unread" : "cached");
  if (message?.error) return `${formatStatus(status)}: ${message.error}`;
  return formatStatus(status);
}

function filteredMessages() {
  const query = state.filter.trim().toLowerCase();
  if (!query) return state.messages;
  return state.messages.filter((message) =>
    [message.from, message.subject, message.preview, message.status]
      .map((value) => text(value).toLowerCase())
      .some((value) => value.includes(query))
  );
}

function toast(message) {
  const node = document.createElement("div");
  node.className = "rounded-md border border-stone-200 bg-white px-3 py-2 text-sm text-stone-700 shadow-soft";
  node.textContent = message;
  toastRoot.append(node);
  setTimeout(() => node.remove(), 3400);
}

function render() {
  const messages = filteredMessages();
  const selected = state.selectedMessage || state.messages.find((item) => item.id === state.selectedId);
  const unreadCount = state.messages.filter((item) => item.unread).length;
  const pendingCount = state.queue.filter((item) => ["queued", "running"].includes(item.status)).length;

  app.innerHTML = `
    <div class="app-shell">
      <header class="col-span-full grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 border-b border-stone-200 bg-white/82 px-4 backdrop-blur">
        <div class="flex items-center gap-3">
          <div class="grid h-9 w-9 place-items-center rounded-md border border-cyan-200 bg-cyan-50 text-cyan-800">
            ${icon("mail")}
          </div>
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <h1 class="truncate text-sm font-semibold">TRUEOS Webmail</h1>
              <span class="rounded-md border border-cyan-200 bg-cyan-50 px-1.5 py-0.5 text-[11px] font-medium text-cyan-800">HTTP</span>
            </div>
            <p class="truncate text-xs text-stone-500">${escapeHtml(statusLine())}</p>
          </div>
        </div>

        <div class="mx-auto w-full max-w-xl">
          <label class="relative block">
            <span class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-stone-400">${icon("search", 16)}</span>
            <input
              id="mail-search"
              class="h-10 w-full rounded-md border border-stone-200 bg-white/80 pl-9 pr-3 text-sm outline-none focus:border-cyan-500"
              placeholder="Search mailbox"
              value="${escapeHtml(state.filter)}"
              type="search"
            />
          </label>
        </div>

        <div class="flex items-center gap-2">
          ${button("refresh", "Refresh", "refresh")}
          ${button("compose", state.composeOpen ? "Close" : "Compose", "square-pen", true)}
          ${button("jobs", "Queue", "list-checks")}
        </div>
      </header>

      <aside class="mail-list-panel min-h-0 border-r border-stone-200 bg-white/46">
        <div class="flex items-center justify-between border-b border-stone-200 px-4 py-3">
          <div>
            <h2 class="text-xs font-semibold uppercase tracking-wide text-stone-500">Mailbox</h2>
            <p class="text-xs text-stone-500">${messages.length} shown / ${state.messages.length} cached</p>
          </div>
          <span class="rounded-md border border-stone-200 bg-white px-2 py-1 text-xs text-stone-600">${unreadCount} unread</span>
        </div>
        <div class="h-[calc(100vh-108px)] overflow-y-auto p-2">
          ${messageListMarkup(messages)}
        </div>
      </aside>

      <main class="relative min-h-0 overflow-hidden">
        <div class="flex h-full flex-col">
          <div class="flex items-center justify-between border-b border-stone-200 bg-white/50 px-4 py-3">
            <nav class="flex min-w-0 items-center gap-2 text-sm text-stone-600">
              <span class="font-medium text-stone-900">Inbox</span>
              ${selected ? `<span class="text-stone-400">/</span><span class="truncate">${escapeHtml(selected.subject || "(no subject)")}</span>` : ""}
            </nav>
            <div class="flex items-center gap-1">
              ${smallStat("U", unreadCount)}
              ${smallStat("C", state.messages.length)}
              ${smallStat("Q", pendingCount)}
            </div>
          </div>

          <section class="min-h-0 flex-1 overflow-y-auto p-5">
            ${state.composeOpen ? composeMarkup() : detailMarkup(selected)}
          </section>
        </div>
      </main>

      <aside class="inspector-panel min-h-0 border-l border-stone-200 bg-white/46 p-4">
        ${inspectorMarkup(selected)}
      </aside>
    </div>

    <section class="queue-drawer fixed bottom-0 left-4 right-4 z-40 rounded-t-md border border-stone-200 bg-white/94 shadow-soft backdrop-blur transition-transform" data-open="${state.queueOpen}">
      <div class="flex h-11 items-center justify-between border-b border-stone-200 px-3">
        <button id="toggle-queue" class="icon-btn inline-flex h-8 w-8 items-center justify-center rounded-md border border-stone-200 bg-white" type="button" title="Toggle queue">
          ${icon(state.queueOpen ? "chevron-down" : "chevron-up", 16)}
        </button>
        <div class="min-w-0 flex-1 px-3">
          <div class="text-sm font-semibold">Webmail Send Queue</div>
          <div class="text-xs text-stone-500">${pendingCount} pending</div>
        </div>
        <button id="clear-queue" class="icon-btn inline-flex h-8 w-8 items-center justify-center rounded-md border border-stone-200 bg-white" type="button" title="Clear finished">
          ${icon("archive-x", 16)}
        </button>
      </div>
      <div class="grid h-[calc(var(--queue-height)-44px)] grid-cols-[minmax(0,1fr)_260px]">
        <div class="overflow-y-auto p-3">${queueMarkup()}</div>
        <div class="border-l border-stone-200 p-4 text-sm text-stone-600">
          <h3 class="mb-2 text-xs font-semibold uppercase tracking-wide text-stone-500">Contract</h3>
          <p>Compose returns a local queued message first.</p>
          <p class="mt-2">SMTP delivery runs through the Rust mail module and updates status in <span class="font-mono">/mail/box.json</span>.</p>
          <pre class="mt-3 rounded-md border border-stone-200 bg-stone-50 p-3 text-xs">GET  /api/webmail/status
GET  /api/webmail/list
GET  /api/webmail/read?id=...
POST /api/webmail/send</pre>
        </div>
      </div>
    </section>
  `;

  bindEvents();
  window.lucide?.createIcons();
}

function button(id, label, iconName, primary = false) {
  const cls = primary
    ? "border-cyan-700 bg-cyan-700 text-white hover:bg-cyan-800"
    : "border-stone-200 bg-white text-stone-700";
  return `
    <button id="${id}-button" class="icon-btn inline-flex h-10 items-center gap-2 rounded-md border px-3 text-sm font-medium ${cls}" type="button">
      ${icon(iconName, 16)}<span>${label}</span>
    </button>
  `;
}

function smallStat(label, value) {
  return `<span class="rounded-md border border-stone-200 bg-white px-2 py-1 text-xs font-medium text-stone-600">${label} ${value}</span>`;
}

function statusLine() {
  if (!state.status) return "Connecting to kernel webmail";
  const account = state.status.account || "account unknown";
  const store = state.status.storePath || "/mail/box.json";
  return `${account} / ${store}`;
}

function messageListMarkup(messages) {
  if (!messages.length) {
    return `<div class="grid h-48 place-items-center rounded-md border border-dashed border-stone-200 text-sm text-stone-500">No messages.</div>`;
  }
  return messages
    .map((message) => `
      <button
        class="message-row mb-2 grid w-full gap-1 rounded-md border border-stone-200 bg-white/58 p-3 text-left"
        data-message-id="${escapeHtml(message.id)}"
        aria-selected="${state.selectedId === message.id}"
        type="button"
      >
        <div class="flex items-start justify-between gap-3">
          <span class="min-w-0 truncate text-sm ${message.unread ? "font-semibold text-stone-950" : "font-medium text-stone-700"}">${escapeHtml(message.from || "Unknown")}</span>
          <span class="shrink-0 text-xs text-stone-500">${escapeHtml(formatDate(message.date))}</span>
        </div>
        <div class="truncate text-sm font-medium text-stone-900">${escapeHtml(message.subject || "(no subject)")}</div>
        <div class="line-clamp-2 text-sm text-stone-500">${escapeHtml(message.preview || "")}</div>
      </button>
    `)
    .join("");
}

function detailMarkup(message) {
  if (!message) {
    return `<div class="grid h-full place-items-center text-sm text-stone-500">Select a message to read.</div>`;
  }
  return `
    <article class="mx-auto grid max-w-4xl gap-5">
      <header class="border-b border-stone-200 pb-5">
        <div class="mb-3 flex items-center justify-between gap-3">
          <h2 class="min-w-0 text-2xl font-semibold text-stone-950">${escapeHtml(message.subject || "(no subject)")}</h2>
          <span class="rounded-md border border-stone-200 bg-white px-2 py-1 text-xs text-stone-600">${escapeHtml(messageStatus(message))}</span>
        </div>
        <div class="grid gap-1 text-sm text-stone-600">
          ${metaRow("From", message.from)}
          ${metaRow("To", message.to)}
          ${metaRow("Date", formatDate(message.date))}
        </div>
      </header>
      <pre class="whitespace-pre-wrap break-words font-sans text-base leading-7 text-stone-800">${escapeHtml(message.body || message.preview || "")}</pre>
    </article>
  `;
}

function metaRow(label, value) {
  return `<div><span class="mr-2 font-medium text-stone-900">${label}:</span><span>${escapeHtml(text(value, "-"))}</span></div>`;
}

function composeMarkup() {
  return `
    <form id="compose-form" class="mx-auto grid max-w-5xl gap-4">
      <div class="flex items-center justify-between gap-3">
        <div>
          <h2 class="text-xl font-semibold">Compose</h2>
          <p class="text-sm text-stone-500">Queued locally, delivered by kernel SMTP.</p>
        </div>
        <button id="discard-button" class="icon-btn inline-flex h-10 items-center gap-2 rounded-md border border-stone-200 bg-white px-3 text-sm font-medium text-stone-700" type="button">
          ${icon("x", 16)}<span>Discard</span>
        </button>
      </div>
      <div class="grid gap-3 sm:grid-cols-2">
        <label class="grid gap-1 text-sm">
          <span class="font-medium text-stone-700">To</span>
          <input id="compose-to" class="rounded-md border border-stone-200 bg-white px-3 py-2 outline-none focus:border-cyan-500" name="to" placeholder="user@example.test" required type="email" />
        </label>
        <label class="grid gap-1 text-sm">
          <span class="font-medium text-stone-700">Subject</span>
          <input id="compose-subject" class="rounded-md border border-stone-200 bg-white px-3 py-2 outline-none focus:border-cyan-500" name="subject" placeholder="Subject" required type="text" />
        </label>
      </div>
      <label class="grid gap-1 text-sm">
        <span class="font-medium text-stone-700">Message</span>
        <textarea id="compose-body" class="min-h-72 rounded-md border border-stone-200 bg-white px-3 py-2 outline-none focus:border-cyan-500" name="body" placeholder="Write a message..." required></textarea>
      </label>
      <div class="flex items-center justify-end gap-2">
        <button class="icon-btn inline-flex h-10 items-center gap-2 rounded-md border border-cyan-700 bg-cyan-700 px-4 text-sm font-medium text-white hover:bg-cyan-800" type="submit">
          ${icon("send", 16)}<span>Send</span>
        </button>
      </div>
    </form>
  `;
}

function inspectorMarkup(message) {
  const status = state.status || {};
  return `
    <div class="grid gap-4">
      <section class="rounded-md border border-stone-200 bg-white/66 p-3">
        <div class="mb-3 flex items-center gap-3">
          <div class="grid h-10 w-10 place-items-center rounded-md border border-cyan-200 bg-cyan-50 text-cyan-800">${icon("server")}</div>
          <div>
            <h2 class="text-sm font-semibold">Webmail</h2>
            <p class="text-xs text-stone-500">${escapeHtml(status.service || "kernel mail")}</p>
          </div>
        </div>
        ${detailRow("Account", status.account)}
        ${detailRow("Store", status.storePath)}
        ${detailRow("SMTP", status.smtp)}
        ${detailRow("POP3", status.pop3)}
      </section>

      <section class="rounded-md border border-stone-200 bg-white/66 p-3">
        <h3 class="mb-2 text-xs font-semibold uppercase tracking-wide text-stone-500">Current Message</h3>
        ${detailRow("From", message?.from)}
        ${detailRow("Subject", message?.subject)}
        ${detailRow("Status", message ? messageStatus(message) : "-")}
      </section>

      <section class="grid grid-cols-3 gap-2">
        ${metric("Cached", state.messages.length)}
        ${metric("Unread", state.messages.filter((item) => item.unread).length)}
        ${metric("Queued", state.queue.filter((item) => ["queued", "running"].includes(item.status)).length)}
      </section>
    </div>
  `;
}

function detailRow(label, value) {
  return `
    <div class="grid grid-cols-[80px_minmax(0,1fr)] border-t border-stone-200 py-2 text-sm first:border-t-0">
      <span class="text-stone-500">${label}</span>
      <span class="truncate font-medium text-stone-800">${escapeHtml(text(value, "-"))}</span>
    </div>
  `;
}

function metric(label, value) {
  return `
    <div class="rounded-md border border-stone-200 bg-white/66 p-3">
      <div class="text-lg font-semibold">${value}</div>
      <div class="text-xs text-stone-500">${label}</div>
    </div>
  `;
}

function queueMarkup() {
  if (!state.queue.length) {
    return `<div class="grid h-full place-items-center rounded-md border border-dashed border-stone-200 text-sm text-stone-500">No queued work.</div>`;
  }
  return state.queue
    .map((job) => `
      <div class="queue-row mb-2 rounded-md border ${job.status === "failed" ? "border-red-200 bg-red-50" : "border-emerald-200 bg-emerald-50"} p-3">
        <div class="flex items-start justify-between gap-3">
          <div>
            <div class="text-sm font-semibold">${escapeHtml(job.label)}</div>
            <div class="text-xs text-stone-600">${escapeHtml(job.description)}</div>
          </div>
          <span class="rounded bg-white px-2 py-1 text-[10px] font-semibold uppercase text-stone-600">${escapeHtml(job.status)}</span>
        </div>
        <div class="mt-3 h-1.5 rounded-full bg-white">
          <div class="progress-bar h-1.5 rounded-full" style="width:${Math.max(0, Math.min(100, job.progress))}%"></div>
        </div>
        <div class="mt-2 font-mono text-xs text-stone-500">${escapeHtml(job.remoteId || job.id)}</div>
      </div>
    `)
    .join("");
}

function bindEvents() {
  document.querySelector("#refresh-button")?.addEventListener("click", () => refreshInbox());
  document.querySelector("#compose-button")?.addEventListener("click", () => {
    state.composeOpen = !state.composeOpen;
    render();
  });
  document.querySelector("#jobs-button")?.addEventListener("click", () => {
    state.queueOpen = !state.queueOpen;
    render();
  });
  document.querySelector("#toggle-queue")?.addEventListener("click", () => {
    state.queueOpen = !state.queueOpen;
    render();
  });
  document.querySelector("#clear-queue")?.addEventListener("click", () => {
    state.queue = state.queue.filter((job) => ["queued", "running"].includes(job.status));
    render();
  });
  document.querySelector("#mail-search")?.addEventListener("input", (event) => {
    state.filter = event.currentTarget.value;
    render();
  });
  document.querySelectorAll("[data-message-id]").forEach((button) => {
    button.addEventListener("click", () => readMessage(button.dataset.messageId));
  });
  document.querySelector("#compose-form")?.addEventListener("submit", sendMessage);
  document.querySelector("#discard-button")?.addEventListener("click", () => {
    state.composeOpen = false;
    render();
  });
}

async function fetchJson(url, options = {}) {
  const response = await fetch(url, {
    headers: {
      Accept: "application/json",
      ...(options.body ? { "Content-Type": "application/json" } : {}),
      ...(options.headers || {}),
    },
    ...options,
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}

async function loadStatus() {
  state.status = await fetchJson(API.status);
}

async function loadMessages() {
  const data = await fetchJson(API.list);
  state.messages = Array.isArray(data.messages) ? data.messages : [];
  if (state.selectedId && !state.messages.some((item) => item.id === state.selectedId)) {
    state.selectedId = null;
    state.selectedMessage = null;
  }
}

async function loadAll() {
  try {
    await Promise.all([loadStatus(), loadMessages()]);
  } catch (error) {
    toast(`Webmail refresh failed: ${error.message || error}`);
  }
  render();
}

async function refreshInbox() {
  try {
    const result = await fetchJson(API.refresh, { method: "POST" });
    if (result.ok === false) toast(`Inbox refresh failed: ${text(result.error, "unknown error")}`);
  } catch (error) {
    toast(`Inbox refresh failed: ${error.message || error}`);
  }
  await loadAll();
}

async function readMessage(id) {
  state.selectedId = id;
  state.selectedMessage = state.messages.find((item) => item.id === id) || null;
  render();
  try {
    const message = await fetchJson(API.read(id));
    state.selectedMessage = { ...(state.selectedMessage || {}), ...message };
  } catch (error) {
    toast(`Read failed: ${error.message || error}`);
  }
  render();
}

function createJob(label) {
  const job = {
    id: `webmail-job-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    label,
    description: "Queued",
    status: "queued",
    progress: 0,
    remoteId: "",
  };
  state.queue.unshift(job);
  state.queueOpen = true;
  render();
  return job;
}

async function sendMessage(event) {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  const payload = {
    to: text(form.get("to")).trim(),
    subject: text(form.get("subject")).trim(),
    body: text(form.get("body")),
  };
  const job = createJob(`Send to ${payload.to}`);
  job.status = "running";
  job.progress = 12;
  job.description = "Submitting to kernel mail";
  render();

  try {
    const result = await fetchJson(API.send, {
      method: "POST",
      body: JSON.stringify(payload),
    });
    if (result.ok === false) throw new Error(text(result.error, "Send failed"));
    job.remoteId = result.id || "";
    job.progress = 100;
    job.status = "succeeded";
    job.description = text(result.status, "queued") === "queued" ? "Queued for SMTP delivery" : "Sent";
    state.composeOpen = false;
    await loadMessages();
  } catch (error) {
    job.progress = 100;
    job.status = "failed";
    job.description = error.message || "Send failed";
  }
  render();
}

render();
loadAll();
setInterval(() => {
  if (document.visibilityState !== "hidden") loadAll();
}, 30_000);
