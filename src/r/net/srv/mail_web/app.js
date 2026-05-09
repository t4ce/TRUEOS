// Static mail frontend endpoint contract:
// GET  /api/mail/list
//      -> { "messages": [{ "id": "abc", "from": "ada@example.test",
//           "subject": "Hello", "preview": "Short body preview",
//           "date": "2026-05-09T10:30:00Z", "unread": true }] }
// GET  /api/mail/read?id=abc
//      -> { "id": "abc", "from": "ada@example.test", "to": "root@trueos",
//           "subject": "Hello", "date": "2026-05-09T10:30:00Z",
//           "body": "Plain text message body" }
// POST /api/mail/send
//      <- { "to": "ada@example.test", "subject": "Re: Hello", "body": "..." }
//      -> { "ok": true, "id": "sent-123" }
const API = {
  list: "/api/mail/list",
  read: (id) => `/api/mail/read?id=${encodeURIComponent(id)}`,
  send: "/api/mail/send",
};

const state = {
  messages: [],
  selectedId: null,
};

const els = {
  status: document.querySelector("#mail-status"),
  count: document.querySelector("#message-count"),
  list: document.querySelector("#message-list"),
  detail: document.querySelector("#message-detail"),
  refresh: document.querySelector("#refresh-button"),
  composeToggle: document.querySelector("#compose-toggle"),
  composePanel: document.querySelector("#compose-panel"),
  composeForm: document.querySelector("#compose-form"),
  composeStatus: document.querySelector("#compose-status"),
  discard: document.querySelector("#discard-button"),
};

const fallbackMessages = [
  {
    id: "demo-welcome",
    from: "postmaster@trueos.local",
    subject: "Mailbox frontend ready",
    preview: "Wire these controls to the TRUEOS mail endpoints when the service is ready.",
    date: new Date().toISOString(),
    unread: true,
  },
];

function text(value, fallback = "") {
  return value === undefined || value === null ? fallback : String(value);
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

function setStatus(message) {
  els.status.textContent = message;
}

function renderList() {
  els.count.textContent = String(state.messages.length);
  els.list.innerHTML = "";

  if (state.messages.length === 0) {
    const empty = document.createElement("div");
    empty.className = "px-4 py-8 text-center text-sm text-zinc-500";
    empty.textContent = "No messages.";
    els.list.append(empty);
    return;
  }

  for (const message of state.messages) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = [
      "grid w-full gap-1 border-b border-zinc-800 px-4 py-3 text-left hover:bg-zinc-900 focus:outline-none focus:ring-2 focus:ring-inset focus:ring-cyan-500",
      state.selectedId === message.id ? "bg-zinc-900" : "bg-transparent",
    ].join(" ");
    button.dataset.id = message.id;

    const top = document.createElement("div");
    top.className = "flex items-start justify-between gap-3";

    const from = document.createElement("span");
    from.className = message.unread
      ? "min-w-0 truncate text-sm font-semibold text-zinc-100"
      : "min-w-0 truncate text-sm font-medium text-zinc-300";
    from.textContent = text(message.from, "Unknown sender");

    const date = document.createElement("span");
    date.className = "shrink-0 text-xs text-zinc-500";
    date.textContent = formatDate(message.date);

    const subject = document.createElement("div");
    subject.className = "truncate text-sm font-medium text-zinc-200";
    subject.textContent = text(message.subject, "(no subject)");

    const preview = document.createElement("div");
    preview.className = "line-clamp-2 text-sm text-zinc-500";
    preview.textContent = text(message.preview);

    top.append(from, date);
    button.append(top, subject, preview);
    button.addEventListener("click", () => readMessage(message.id));
    els.list.append(button);
  }
}

function renderDetail(message) {
  els.detail.innerHTML = "";

  const shell = document.createElement("div");
  shell.className = "mx-auto grid max-w-3xl gap-5";

  const header = document.createElement("header");
  header.className = "grid gap-2 border-b border-zinc-800 pb-5";

  const subject = document.createElement("h2");
  subject.className = "text-2xl font-semibold text-zinc-50";
  subject.textContent = text(message.subject, "(no subject)");

  const meta = document.createElement("div");
  meta.className = "grid gap-1 text-sm text-zinc-400";
  meta.append(metaRow("From", message.from), metaRow("To", message.to), metaRow("Date", formatDate(message.date)));

  const body = document.createElement("pre");
  body.className = "whitespace-pre-wrap break-words font-sans text-base leading-7 text-zinc-200";
  body.textContent = text(message.body, text(message.preview, ""));

  header.append(subject, meta);
  shell.append(header, body);
  els.detail.append(shell);
}

function metaRow(label, value) {
  const row = document.createElement("div");
  const strong = document.createElement("span");
  strong.className = "mr-2 font-medium text-zinc-300";
  strong.textContent = `${label}:`;
  const span = document.createElement("span");
  span.textContent = text(value, "-");
  row.append(strong, span);
  return row;
}

async function loadMessages() {
  setStatus("Refreshing mailbox...");
  try {
    const response = await fetch(API.list, { headers: { Accept: "application/json" } });
    if (!response.ok) throw new Error(`List request failed with ${response.status}`);
    const data = await response.json();
    state.messages = Array.isArray(data.messages) ? data.messages : [];
    setStatus("Mailbox loaded.");
  } catch (error) {
    state.messages = fallbackMessages;
    setStatus(`Using demo data: ${error.message}`);
  }
  renderList();
}

async function readMessage(id) {
  state.selectedId = id;
  renderList();
  setStatus("Opening message...");

  const summary = state.messages.find((message) => message.id === id) || {};
  try {
    const response = await fetch(API.read(id), { headers: { Accept: "application/json" } });
    if (!response.ok) throw new Error(`Read request failed with ${response.status}`);
    const message = await response.json();
    renderDetail({ ...summary, ...message });
    setStatus("Message opened.");
  } catch (error) {
    renderDetail({
      ...summary,
      body: `${text(summary.preview, "No cached body available.")}\n\n${error.message}`,
    });
    setStatus("Showing cached message summary.");
  }
}

async function sendMessage(event) {
  event.preventDefault();
  const form = new FormData(els.composeForm);
  const payload = {
    to: text(form.get("to")).trim(),
    subject: text(form.get("subject")).trim(),
    body: text(form.get("body")),
  };

  els.composeStatus.textContent = "Sending...";
  try {
    const response = await fetch(API.send, {
      method: "POST",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    });
    if (!response.ok) throw new Error(`Send request failed with ${response.status}`);
    const result = await response.json();
    if (result.ok === false) throw new Error(text(result.error, "Send failed"));
    els.composeForm.reset();
    els.composeStatus.textContent = "Message sent.";
    await loadMessages();
  } catch (error) {
    els.composeStatus.textContent = error.message;
  }
}

els.refresh.addEventListener("click", loadMessages);
els.composeToggle.addEventListener("click", () => {
  els.composePanel.classList.toggle("hidden");
});
els.discard.addEventListener("click", () => {
  els.composeForm.reset();
  els.composeStatus.textContent = "";
  els.composePanel.classList.add("hidden");
});
els.composeForm.addEventListener("submit", sendMessage);

loadMessages();
