const qs = (id) => document.getElementById(id);

const state = {
  messages: [],
  selected: null,
};

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: { "content-type": "application/json" },
    ...options,
  });
  return response.json();
}

function setStatus(text, kind = "muted") {
  const el = qs("status");
  el.className = kind;
  el.textContent = text;
}

async function loadStatus() {
  const status = await api("/api/webmail/status");
  const configured = status.passwordConfigured ? "ready" : "needs password";
  setStatus(`${status.account} · ${configured} · ${status.smtp}`, status.passwordConfigured ? "ok" : "warn");
}

async function loadConfig() {
  const result = await api("/api/webmail/config");
  if (!result.ok) {
    qs("config-state").textContent = result.error || "Config unavailable";
    return;
  }
  const config = result.config;
  qs("smtp-user").value = config.smtp_user || "";
  qs("smtp-pass").value = "";
  qs("config-state").textContent = config.passwordConfigured
    ? `Password configured in ${result.source}`
    : "Enter the mailbox password for this boot/config.";
}

async function saveConfig() {
  const payload = {
    smtp_user: qs("smtp-user").value,
    smtp_pass: qs("smtp-pass").value,
  };
  const result = await api("/api/webmail/config", {
    method: "POST",
    body: JSON.stringify(payload),
  });
  if (!result.ok) {
    qs("config-state").textContent = result.error || "Save failed";
    return;
  }
  qs("smtp-pass").value = "";
  qs("config-state").textContent = result.config.passwordConfigured
    ? "Password saved."
    : "Password cleared.";
  await loadStatus();
}

function renderMessages() {
  const list = qs("messages");
  list.textContent = "";
  for (const message of state.messages) {
    const item = document.createElement("button");
    item.type = "button";
    item.className = `message secondary${state.selected === message.id ? " active" : ""}`;
    item.innerHTML = `<strong>${message.subject || "(no subject)"}</strong><br><span class="muted">${message.from} · ${message.status}</span><br>${message.preview || ""}`;
    item.addEventListener("click", () => readMessage(message.id));
    list.appendChild(item);
  }
}

async function loadMessages() {
  const result = await api("/api/webmail/list");
  state.messages = result.messages || [];
  renderMessages();
}

async function readMessage(id) {
  const message = await api(`/api/webmail/read?id=${encodeURIComponent(id)}`);
  state.selected = id;
  renderMessages();
  qs("reader").className = "panel stack";
  qs("reader").innerHTML = `<h2>${message.subject || "(no subject)"}</h2><div class="muted">${message.from} → ${message.to}</div><pre>${message.body || ""}</pre>`;
}

async function sendMessage(event) {
  event.preventDefault();
  const payload = {
    to: qs("to").value,
    subject: qs("subject").value,
    body: qs("body").value,
  };
  const result = await api("/api/webmail/send", {
    method: "POST",
    body: JSON.stringify(payload),
  });
  if (!result.ok) {
    setStatus(result.error || "Send failed", "warn");
    return;
  }
  setStatus(`Queued ${result.id}`, "ok");
  qs("compose-form").reset();
  await loadMessages();
}

async function refreshInbox() {
  const result = await api("/api/webmail/refresh", { method: "POST" });
  setStatus(result.ok ? `Refresh queued, added ${result.added || 0}` : result.error || "Refresh failed", result.ok ? "ok" : "warn");
  await loadMessages();
}

qs("save-config").addEventListener("click", saveConfig);
qs("config-form").addEventListener("submit", (event) => {
  event.preventDefault();
  saveConfig();
});
qs("compose-form").addEventListener("submit", sendMessage);
qs("refresh").addEventListener("click", refreshInbox);

Promise.all([loadStatus(), loadConfig(), loadMessages()]).catch((error) => {
  setStatus(error.message || "Webmail failed", "warn");
});
