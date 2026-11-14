/* TRUEOS Cloud Build. Private keys are never sent; bearer tokens stay in memory. */
"use strict";

(() => {
  // Resolve against the loaded script so root and /build/ installations share
  // the same UI without sending requests to another application's /api route.
  const apiBase = new URL(".", document.currentScript.src).pathname.replace(/\/$/, "");
  const apiURL = (path) => apiBase + path;
  const $ = (id) => document.getElementById(id);
  const state = {
    catalog: null,
    capabilities: { anonymous: false, licensed: false },
    selected: new Set(),
    selectedBuildins: new Set(),
    activePreset: null,
    basePreset: null,
    profile: "release",
    manifest: null,
    sourceCommit: "",
    namespace: "build-request@trueos.eu",
    configRevision: 0,
    requestRevision: 0,
    planTimer: null,
    planController: null,
    challenge: null,
    expiryTimer: null,
    challengeDeadline: 0,
    challengeExpired: false,
    proofVerified: false,
    job: null,
    jobRequestRevision: -1,
    jobGeneration: 0,
    jobTimer: null,
    jobController: null,
    retryKeyRequired: false,
    busy: false,
  };

  const iconPaths = {
    buildins: '<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/>',
    kernel: '<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 3v3m6-3v3M9 18v3m6-3v3M3 9h3m-3 6h3m12-6h3m-3 6h3M10 10h4v4h-4z"/>',
    usb: '<path d="M12 21V3m0 0-3 3m3-3 3 3M12 15l-5-4V8m5 10 5-4v-4"/><circle cx="7" cy="6" r="2"/><rect x="15" y="6" width="4" height="4" rx=".5"/>',
    igpu: '<rect x="6" y="6" width="12" height="12" rx="2"/><rect x="9" y="9" width="6" height="6" rx="1"/><path d="M9 3v3m6-3v3M9 18v3m6-3v3M3 9h3m-3 6h3m12-6h3m-3 6h3"/>',
    virgl: '<path d="m12 3 8 4.5v9L12 21l-8-4.5v-9L12 3Zm0 9 8-4.5M12 12 4 7.5M12 12v9m-4-6.7 8-4.5"/>',
    h264: '<rect x="3" y="5" width="13" height="14" rx="2"/><path d="m16 10 5-3v10l-5-3M8 9l4 3-4 3V9Z"/>',
    task_profile: '<path d="M4 18h16M6 14v-4m6 4V5m6 9v-7"/><circle cx="6" cy="10" r="1"/><circle cx="12" cy="5" r="1"/><circle cx="18" cy="7" r="1"/>',
    native_gpgpu: '<path d="m13 2-9 12h7l-1 8 10-13h-7l1-7Z"/>',
    speech: '<rect x="9" y="3" width="6" height="12" rx="3"/><path d="M5 10v2a7 7 0 0 0 14 0v-2M12 19v3m-4 0h8"/>',
    universal: '<path d="m12 3 9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5"/>',
  };

  function make(tag, className, text) {
    const element = document.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined) element.textContent = text;
    return element;
  }

  function icon(id, className) {
    const element = make("span", className);
    element.setAttribute("aria-hidden", "true");
    // Only fixed, repository-owned SVG paths are inserted as markup.
    element.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">${iconPaths[id] || iconPaths.universal}</svg>`;
    return element;
  }

  function labelFor(id) {
    const item = state.catalog?.features.find((feature) => feature.id === id);
    return item?.label || item?.name || id;
  }

  function kernelFeatures() {
    return state.catalog.features.filter((feature) => feature.id !== "buildins");
  }

  function buildinLabelFor(id) {
    const item = state.catalog?.buildins.find((buildin) => buildin.id === id);
    return item?.label || id;
  }

  function byteCount(value) {
    return Number.isSafeInteger(value) && value >= 0 ? value : null;
  }

  function sizeMetadata() {
    const data = state.catalog?.size_estimates;
    return data?.schema === 1 ? data : null;
  }

  function formatKilobytes(bytes) {
    return bytes === null ? "Unmeasured" : `${Math.round(bytes / 1000).toLocaleString("en-US")} kB`;
  }

  function sizeMetrics(appIds) {
    const data = sizeMetadata();
    const allIds = state.catalog.buildins.map(({ id }) => id);
    const known = new Set(allIds);
    const selected = new Set(appIds);
    function sum(ids) {
      let bytes = 0;
      for (const id of ids) {
        const value = byteCount(data?.buildins?.[id]?.packed_bp_bytes);
        if (!known.has(id) || value === null) return null;
        bytes += value;
        if (!Number.isSafeInteger(bytes)) return null;
      }
      return bytes;
    }
    // Accumulate original bytes, then round for display. Never sum rounded badges.
    const selectedBytes = sum(selected);
    const omittedBytes = sum(allIds.filter((id) => !selected.has(id)));
    const kernelBytes = byteCount(data?.kernel?.base_bytes);
    const totalBytes = kernelBytes !== null && selectedBytes !== null
      ? byteCount(kernelBytes + selectedBytes) : null;
    return { data, selectedBytes, omittedBytes, kernelBytes, totalBytes, allBytes: sum(allIds) };
  }

  function sizeValue(id, bytes, estimated = false) {
    const element = $(id);
    element.textContent = `${estimated && bytes !== null && bytes > 0 ? "≈ " : ""}${formatKilobytes(bytes)}`;
    if (bytes !== null) element.title = `${bytes.toLocaleString("en-US")} bytes${estimated ? " (estimate)" : " in the reference snapshot"}`;
    else element.removeAttribute("title");
  }

  function referenceProfile(data) {
    return data?.reference?.profile === "dev" ? "Development" : data?.reference?.profile === "release" ? "Release" : "Reference";
  }

  function renderSizeEstimates() {
    const metrics = sizeMetrics(state.selectedBuildins);
    const { data, selectedBytes, omittedBytes, kernelBytes, totalBytes, allBytes } = metrics;
    const reference = data?.reference;
    const profile = referenceProfile(data);
    const profileDiffers = Boolean(reference?.profile && reference.profile !== state.profile);
    const sourceDiffers = Boolean(reference?.source_commit && reference.source_commit !== state.sourceCommit);
    $("size-heading").textContent = profileDiffers ? `${profile} size reference` : "Image size estimate";
    $("size-total-label").textContent = profileDiffers ? `${profile} comparison total` : "Estimated kernel + apps";
    $("size-reference").textContent = reference
      ? `${reference.label || "Measured snapshot"} · ${profile.toLowerCase()} snapshot`
      : "Reference measurements are unavailable.";
    $("size-kernel-label").textContent = `${data?.kernel?.label || "Kernel baseline"}${data?.kernel?.basis === "derived" ? " (estimated)" : ""}`;
    sizeValue("size-kernel", kernelBytes, data?.kernel?.basis === "derived");
    sizeValue("size-apps", selectedBytes);
    sizeValue("size-total", totalBytes, true);
    sizeValue("size-saved", omittedBytes);
    // Subsystem deltas require their own measured builds; no deduction is inferred.
    $("size-subsystems").textContent = "Not measured";
    $("size-subsystems-note").textContent = data?.subsystems?.note || "Subsystem reductions have not been measured. No subsystem byte savings are deducted.";
    $("size-method-note").textContent = data?.kernel?.note || "Kernel baseline measurements are unavailable.";
    sizeValue("size-reference-kernel", byteCount(reference?.kernel_elf_bytes));
    sizeValue("size-reference-download", byteCount(reference?.archive_bytes));

    const context = [];
    if (reference) {
      if (profileDiffers) context.push(`${state.profile === "dev" ? "Development" : "Release"} selected. These ${profile.toLowerCase()} figures are a comparison reference; the selected profile has not been measured.`);
      if (!reference.source_commit) context.push("This snapshot has no verified source revision; rebuilt sizes may vary.");
      else if (sourceDiffers) context.push("The selected source differs from this snapshot; rebuilt sizes may vary.");
      else context.push("Snapshot measurements are a guide; rebuilt sizes may vary.");
    }
    $("size-context").textContent = context.join(" ");
    $("size-context").hidden = !context.length;

    const hasMeter = allBytes !== null && allBytes > 0 && selectedBytes !== null && omittedBytes !== null;
    $("size-meter").hidden = !hasMeter;
    $("size-meter-caption").hidden = !hasMeter;
    if (hasMeter) {
      const percent = Math.min(100, Math.max(0, 100 * selectedBytes / allBytes));
      $("size-meter-selected").style.width = `${percent}%`;
      $("size-meter").setAttribute("aria-valuenow", percent.toFixed(1));
      $("size-meter").setAttribute("aria-valuetext", `${formatKilobytes(selectedBytes)} of app payload included; ${formatKilobytes(omittedBytes)} omitted`);
      $("size-meter-caption").textContent = `${Math.round(percent)}% of reference app payload included`;
    } else {
      $("size-meter").removeAttribute("aria-valuenow");
      $("size-meter").removeAttribute("aria-valuetext");
      $("size-meter-selected").style.width = "0%";
      $("size-meter-caption").textContent = "";
    }
  }

  function presetBuildins(preset) {
    return Array.isArray(preset.buildins) ? preset.buildins
      : preset.features.includes("buildins") ? state.catalog.buildins.map(({ id }) => id) : [];
  }

  function applyPreset(preset) {
    state.selected = new Set(preset.features.filter((id) => id !== "buildins"));
    state.selectedBuildins = new Set(presetBuildins(preset));
    state.activePreset = preset.id;
    state.basePreset = preset.id;
  }

  function notify(message, error = false) {
    $("notice").textContent = message;
    $("notice").className = error ? "notice error" : "notice";
    $("notice").hidden = !message;
  }

  function fieldError(id, message, inputId) {
    $(id).textContent = message;
    $(id).hidden = !message;
    if (inputId) $(inputId).setAttribute("aria-invalid", message ? "true" : "false");
  }

  async function api(path, { method = "GET", body, token, signal } = {}) {
    const headers = { Accept: "application/json" };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (token) headers.Authorization = `Bearer ${token}`;
    const response = await fetch(apiURL(path), {
      method, headers, signal, credentials: "omit", mode: "same-origin", cache: "no-store",
      ...(body !== undefined ? { body: JSON.stringify(body) } : {}),
    });
    let data;
    try { data = await response.json(); }
    catch { throw new Error(`The server returned an unreadable response (${response.status}).`); }
    if (!response.ok) {
      const message = typeof data.error === "string" ? data.error : data.error?.message || data.message;
      const error = new Error(message || `Request failed (${response.status}).`);
      error.status = response.status;
      throw error;
    }
    return data;
  }

  function requestSpec() {
    return {
      preset: state.basePreset,
      features: Object.fromEntries(kernelFeatures().map(({ id }) => [id, state.selected.has(id)])),
      buildins: [...state.selectedBuildins].sort(),
      profile: state.profile,
    };
  }

  function renderCatalog() {
    $("presets").replaceChildren(make("legend", "sr-only", "Build presets; selecting a preset replaces the current selection"));
    for (const preset of state.catalog.presets) {
      const card = make("label", "preset-card");
      card.dataset.preset = preset.id;
      const checkbox = make("input");
      checkbox.type = "checkbox";
      checkbox.name = "preset";
      checkbox.value = preset.id;
      checkbox.setAttribute("aria-describedby", `preset-description-${preset.id}`);
      checkbox.addEventListener("change", () => {
        if (state.busy) return;
        if (checkbox.checked) {
          applyPreset(preset);
        } else {
          state.activePreset = null;
        }
        configurationChanged();
      });
      const description = make("span", "preset-description", preset.description || "Customize this preset below.");
      description.id = `preset-description-${preset.id}`;
      const iconId = preset.id === "kernel-only" ? "kernel" : preset.id.includes("intel") ? "igpu" : preset.id.includes("virgl") || preset.id.includes("qemu") ? "virgl" : "universal";
      card.append(icon(iconId, "preset-icon"), checkbox, make("span", "preset-label", preset.label || preset.name || preset.id), description);
      $("presets").append(card);
    }

    $("features").replaceChildren(make("legend", "sr-only", "Kernel subsystem switches"));
    for (const feature of kernelFeatures()) {
      const card = make("label", "feature-card");
      card.dataset.feature = feature.id;
      const text = make("span", "feature-copy");
      const description = make("span", "feature-description", feature.description || "");
      description.id = `feature-description-${feature.id}`;
      text.append(make("span", "feature-label", feature.label || feature.name || feature.id), description);
      if (feature.requires?.length) text.append(make("span", "feature-requires", `Requires ${feature.requires.map(labelFor).join(" + ")}`));
      const checkbox = make("input");
      checkbox.type = "checkbox";
      checkbox.name = "feature";
      checkbox.value = feature.id;
      checkbox.setAttribute("aria-describedby", description.id);
      checkbox.addEventListener("change", () => changeFeature(feature.id, checkbox.checked));
      card.append(icon(feature.id, "feature-icon"), text, checkbox);
      $("features").append(card);
    }

    const sizeData = sizeMetadata();
    $("buildin-size-basis").textContent = sizeData
      ? `Packed app sizes from ${sizeData.reference?.label || "the measured snapshot"}. kB = 1,000 bytes.`
      : "Package size measurements are unavailable.";
    $("buildins").replaceChildren(make("legend", "sr-only", "Individual bundled apps"));
    for (const buildin of state.catalog.buildins) {
      const card = make("label", "buildin-card");
      card.dataset.buildin = buildin.id;
      const checkbox = make("input");
      checkbox.type = "checkbox";
      checkbox.name = "buildin";
      checkbox.value = buildin.id;
      const description = make("span", "buildin-description", buildin.description || "");
      description.id = `buildin-description-${buildin.id}`;
      checkbox.setAttribute("aria-describedby", description.id);
      checkbox.addEventListener("change", () => {
        if (state.busy) return;
        if (checkbox.checked) state.selectedBuildins.add(buildin.id);
        else state.selectedBuildins.delete(buildin.id);
        state.activePreset = null;
        configurationChanged();
      });
      const text = make("span", "buildin-copy");
      const heading = make("span", "buildin-heading");
      const bytes = byteCount(sizeData?.buildins?.[buildin.id]?.packed_bp_bytes);
      const size = make("span", "buildin-size", formatKilobytes(bytes));
      size.id = `buildin-size-${buildin.id}`;
      if (bytes !== null) size.title = `${bytes.toLocaleString("en-US")} packed package bytes in the reference snapshot`;
      checkbox.setAttribute("aria-describedby", `${description.id} ${size.id}`);
      heading.append(make("span", "buildin-label", buildin.label || buildin.id), size);
      text.append(heading, description);
      card.append(checkbox, text);
      $("buildins").append(card);
    }
  }

  function selectBuildins(all) {
    if (!state.catalog || state.busy) return;
    state.selectedBuildins = new Set(all ? state.catalog.buildins.map(({ id }) => id) : []);
    state.activePreset = null;
    configurationChanged();
  }

  function updateBuildinShortcuts() {
    $("buildins-all").disabled = state.busy || !state.catalog || state.selectedBuildins.size === state.catalog.buildins.length;
    $("buildins-none").disabled = state.busy || !state.selectedBuildins.size;
  }

  function changeFeature(id, enabled) {
    if (state.busy) return;
    const adjusted = [];
    const visited = new Set();
    function enable(featureId) {
      if (visited.has(featureId)) return;
      visited.add(featureId);
      const feature = state.catalog.features.find((item) => item.id === featureId);
      for (const requirement of feature?.requires || []) enable(requirement);
      if (!state.selected.has(featureId)) {
        state.selected.add(featureId);
        if (featureId !== id) adjusted.push(labelFor(featureId));
      }
    }
    function disable(featureId) {
      if (visited.has(featureId)) return;
      visited.add(featureId);
      if (state.selected.delete(featureId) && featureId !== id) adjusted.push(labelFor(featureId));
      for (const feature of state.catalog.features) {
        if (feature.requires?.includes(featureId)) disable(feature.id);
      }
    }
    if (enabled) enable(id); else disable(id);
    state.activePreset = null;
    configurationChanged();
    if (adjusted.length) notify(enabled
      ? `Also enabled ${adjusted.join(", ")} to satisfy this feature’s requirements.`
      : `Also disabled ${adjusted.join(", ")} because ${labelFor(id)} is required.`);
  }

  function clearRequest() {
    state.requestRevision += 1;
    clearTimeout(state.expiryTimer);
    state.challenge = null;
    state.challengeExpired = false;
    state.proofVerified = false;
    $("challenge-panel").hidden = true;
    $("signature").value = "";
    $("signature-file").value = "";
    $("signature-file-name").textContent = "Or paste the signature above.";
    fieldError("signature-error", "", "signature");
  }

  function configurationChanged() {
    state.configRevision += 1;
    state.retryKeyRequired = false;
    clearRequest();
    notify("");
    renderSelection();
    schedulePlan();
  }

  function renderSelection() {
    for (const card of document.querySelectorAll("[data-preset]")) {
      const selected = card.dataset.preset === state.activePreset;
      card.classList.toggle("selected", selected);
      card.querySelector("input").checked = selected;
    }
    for (const card of document.querySelectorAll("[data-feature]")) {
      const selected = state.selected.has(card.dataset.feature);
      card.classList.toggle("enabled", selected);
      card.querySelector("input").checked = selected;
    }
    for (const card of document.querySelectorAll("[data-buildin]")) {
      const selected = state.selectedBuildins.has(card.dataset.buildin);
      card.classList.toggle("enabled", selected);
      card.querySelector("input").checked = selected;
    }
    for (const id of ["usb", "igpu", "virgl"]) $("diagram-" + id).classList.toggle("enabled", state.selected.has(id));
    const count = state.selected.size;
    const appCount = state.selectedBuildins.size;
    $("diagram-count").textContent = `${count} features · ${appCount} apps`;
    $("feature-count").textContent = `${count} SELECTED`;
    $("buildin-count").textContent = `${appCount} / ${state.catalog.buildins.length} SELECTED`;
    $("preset-tag").textContent = state.activePreset ? "PRESETS" : "CUSTOM";
    const preset = state.catalog.presets.find((item) => item.id === state.activePreset);
    $("summary-preset").textContent = preset ? preset.label || preset.name || preset.id : "Custom selection";
    $("summary-profile").textContent = state.profile === "dev" ? "Development" : "Release";
    $("summary-buildins").textContent = appCount ? `${appCount} of ${state.catalog.buildins.length} selected` : "None · Kernel only";
    $("selected-buildins").textContent = `Selected apps: ${state.catalog.buildins.filter(({ id }) => state.selectedBuildins.has(id)).map(({ id }) => buildinLabelFor(id)).join(", ")}`;
    $("selected-buildins").hidden = !appCount;
    $("buildins-note").hidden = appCount > 0;
    $("profile-description").textContent = state.profile === "dev" ? "Development diagnostics enabled." : "Optimized for everyday use.";
    $("selected-features").replaceChildren();
    for (const feature of kernelFeatures()) {
      if (state.selected.has(feature.id)) $("selected-features").append(make("span", "feature-chip", feature.label || feature.name || feature.id));
    }
    if (!count) $("selected-features").append(make("span", "empty-selection", "No optional features selected."));
    renderSizeEstimates();
    updateBuildinShortcuts();
    updateIdentity();
  }

  function planStatus(text, kind) {
    $("plan-state").textContent = text;
    $("plan-state").className = `pill ${kind}`;
  }

  function schedulePlan(immediate = false) {
    clearTimeout(state.planTimer);
    state.planController?.abort();
    state.manifest = null;
    $("config-hash").textContent = "Resolving…";
    $("config-hash").removeAttribute("title");
    $("manifest-json").textContent = "Resolving configuration…";
    $("plan-warnings").hidden = true;
    fieldError("plan-error", "");
    planStatus("RESOLVING", "neutral");
    updateActions();
    state.planTimer = setTimeout(resolvePlan, immediate ? 0 : 200);
  }

  async function resolvePlan() {
    const revision = state.configRevision;
    const controller = new AbortController();
    state.planController = controller;
    try {
      const manifest = await api("/api/plan", { method: "POST", body: requestSpec(), signal: controller.signal });
      if (revision !== state.configRevision || controller !== state.planController) return;
      if (!Array.isArray(manifest.features) || !Array.isArray(manifest.buildins) || typeof manifest.config_hash !== "string") throw new Error("The server returned an incomplete build manifest.");
      state.manifest = manifest;
      $("config-hash").textContent = manifest.config_hash.slice(0, 12);
      $("config-hash").title = manifest.config_hash;
      $("manifest-json").textContent = JSON.stringify(manifest, null, 2);
      $("plan-warnings").replaceChildren();
      for (const warning of manifest.warnings || []) $("plan-warnings").append(make("li", "", String(warning)));
      $("plan-warnings").hidden = !(manifest.warnings?.length);
      planStatus("VALID", "valid");
    } catch (error) {
      if (error.name === "AbortError" || revision !== state.configRevision || controller !== state.planController) return;
      state.manifest = null;
      $("config-hash").textContent = "—";
      $("manifest-json").textContent = "No valid manifest. Resolve the configuration error above.";
      fieldError("plan-error", error.message);
      planStatus("INVALID", "invalid");
    } finally {
      if (revision === state.configRevision && controller === state.planController) updateActions();
    }
  }

  function privateKeyLike(value) {
    return /-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----/.test(value) || /^PuTTY-User-Key-File-/m.test(value);
  }

  function keyError() {
    const value = $("public-key").value.trim();
    if (!value) return "";
    if (privateKeyLike(value)) return "A private key cannot be submitted. Use the public .pub key instead.";
    if (/[\r\n]/.test(value)) return "Paste one public key on one line.";
    if (!/^ssh-ed25519[ \t]+[A-Za-z0-9+/]+={0,2}(?:[ \t]+[^\r\n]*)?$/.test(value)) return "Use an SSH Ed25519 public key beginning with ssh-ed25519.";
    return "";
  }

  function signatureError() {
    const value = $("signature").value.trim();
    if (!value) return "Paste or upload the detached signature first.";
    if (privateKeyLike(value)) return "A private key cannot be submitted. Use the generated .sig file.";
    if (value.length > 16384) return "The signature exceeds the 16 KB limit.";
    if (!/^-----BEGIN SSH SIGNATURE-----[\r\n]+[A-Za-z0-9+/=\r\n]+-----END SSH SIGNATURE-----$/.test(value)) return "Use the complete SSH signature, including its BEGIN and END lines.";
    return "";
  }

  function updateIdentity() {
    const hasKey = Boolean($("public-key").value.trim());
    $("summary-identity").textContent = state.proofVerified ? "Key verified" : hasKey ? "Signature required" : state.capabilities.anonymous ? "Anonymous" : "No key supplied";
  }

  function setBusy(busy) {
    state.busy = busy;
    document.body.classList.toggle("busy", busy);
    for (const input of document.querySelectorAll("#presets input, #features input, #buildins input, #profile-controls input, #public-key, #signature, #signature-file, #resume-file")) input.disabled = busy;
    updateBuildinShortcuts();
    updateActions();
  }

  function updateActions() {
    const hasKey = Boolean($("public-key").value.trim());
    let label = hasKey ? "Create signing challenge" : "Queue build";
    let hint = hasKey ? "Download a challenge and sign it locally. Your private key never leaves your machine." : "This request will be queued without a key-bound license receipt.";
    let enabled = Boolean(state.manifest) && !state.busy;
    if (!state.catalog) { label = "Connecting to build service"; enabled = false; }
    else if (!state.manifest) { label = "Resolve configuration first"; enabled = false; }
    else if (state.job && state.jobRequestRevision === state.requestRevision) { label = "Build requested"; enabled = false; hint = "Follow your request below. Save its private access file to resume later."; }
    else if (hasKey && keyError()) { label = "Check your public key"; enabled = false; hint = "Use a single SSH Ed25519 public key from your .pub file."; }
    else if (hasKey && !state.capabilities.licensed) { label = "Key-bound builds unavailable"; enabled = false; hint = "The operator has not enabled licensed builds on this server."; }
    else if (!hasKey && state.retryKeyRequired) { label = "Add a public key to retry"; enabled = false; hint = "This was a key-bound request. Add your public key and sign a fresh challenge to retry it."; }
    else if (!hasKey && !state.capabilities.anonymous) { label = state.capabilities.licensed ? "Add a public key to continue" : "Build service not configured"; enabled = false; hint = state.capabilities.licensed ? "Anonymous builds are disabled. Add an entitled public key to request a build." : "You can explore and download configurations. The operator must enable a build policy before requests can be queued."; }
    else if (state.challenge && !state.challengeExpired) { label = "Challenge ready to sign"; enabled = false; hint = "Complete the signature step below your public key, then verify to queue your build."; }
    else if (state.challengeExpired) { label = "Create a new challenge"; hint = "The previous challenge expired. Create a fresh one and sign the new file."; }
    if (state.job && state.jobRequestRevision !== state.requestRevision) hint += " Save the earlier build’s private access file before queuing another request.";
    if (state.busy) label = "Working…";
    $("build-button-label").textContent = label;
    $("build-button").disabled = !enabled;
    $("build-hint").textContent = hint;
    $("download-manifest").disabled = !state.manifest;
    $("verify-button").disabled = state.busy || !state.challenge || state.challengeExpired || Boolean(signatureError()) || (Boolean(state.job) && state.jobRequestRevision === state.requestRevision);
    $("verify-button").textContent = state.busy ? "Verifying & queuing…" : "Verify key & queue build ↗";
    $("download-challenge").disabled = state.busy || !state.challenge || state.challengeExpired;
    updateIdentity();
  }

  function download(content, filename, type = "application/json") {
    const blob = content instanceof Blob ? content : new Blob([content], { type });
    const url = URL.createObjectURL(blob);
    const link = make("a");
    link.href = url;
    link.download = filename.replace(/[\\/\u0000]/g, "-");
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 10000);
  }

  function expiryDate(value) {
    return typeof value === "number" ? new Date(value < 1e12 ? value * 1000 : value) : new Date(value);
  }

  function showChallenge(challenge) {
    const expires = expiryDate(challenge.expires_at);
    if (typeof challenge.challenge !== "string" || !challenge.challenge_id || !Number.isFinite(expires.getTime())) throw new Error("The server returned an incomplete signing challenge.");
    if (challenge.manifest?.config_hash !== state.manifest.config_hash) throw new Error("The challenge does not match the reviewed configuration. Resolve the configuration again before signing.");
    state.challenge = challenge;
    const ttl = Number.isFinite(challenge.ttl_seconds) ? Math.max(0, Math.min(300, challenge.ttl_seconds)) : 300;
    state.challengeDeadline = performance.now() + ttl * 1000;
    state.challengeExpired = ttl === 0;
    $("challenge-panel").hidden = false;
    $("challenge-state").textContent = state.challengeExpired ? "EXPIRED" : "READY TO SIGN";
    $("challenge-state").className = state.challengeExpired ? "pill invalid" : "pill";
    $("key-fingerprint").textContent = challenge.fingerprint || "Unknown fingerprint";
    $("challenge-expiry").title = `Server expiry: ${expires.toISOString()}`;
    $("sign-command").textContent = `ssh-keygen -Y sign -f ~/.ssh/id_ed25519 -n ${state.namespace} trueos-challenge.json`;
    $("signature").value = "";
    $("signature-file").value = "";
    $("signature-file-name").textContent = "Or paste the signature above.";
    fieldError("signature-error", "", "signature");
    clearTimeout(state.expiryTimer);
    updateChallengeExpiry();
    $("challenge-heading").focus({ preventScroll: true });
    $("challenge-panel").scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  function updateChallengeExpiry() {
    clearTimeout(state.expiryTimer);
    if (!state.challenge || state.proofVerified) return;
    const seconds = Math.max(0, Math.ceil((state.challengeDeadline - performance.now()) / 1000));
    $("challenge-expiry").textContent = seconds ? `In ${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s` : "Expired — request a new challenge";
    if (!seconds) {
      state.challengeExpired = true;
      $("challenge-state").textContent = "EXPIRED";
      $("challenge-state").className = "pill invalid";
      updateActions();
    } else state.expiryTimer = setTimeout(updateChallengeExpiry, 1000);
  }

  async function startRequest() {
    if (state.busy || !state.manifest || $("build-button").disabled) return;
    const revision = state.requestRevision;
    const publicKey = $("public-key").value.trim();
    if (keyError()) { fieldError("key-error", keyError(), "public-key"); return; }
    notify(state.job ? "Your earlier build remains below. Save its private access file before queuing a new request." : "");
    setBusy(true);
    try {
      if (publicKey) {
        const challenge = await api("/api/challenges", { method: "POST", body: { request: requestSpec(), public_key: publicKey } });
        if (revision !== state.requestRevision) return;
        showChallenge(challenge);
      } else {
        const job = await api("/api/builds", { method: "POST", body: { request: requestSpec() } });
        if (revision !== state.requestRevision) return;
        acceptJob(job);
      }
    } catch (error) { if (revision === state.requestRevision) notify(error.message, true); }
    finally { setBusy(false); }
  }

  async function verifyAndQueue() {
    if (state.busy || !state.challenge || state.challengeExpired) return;
    const errorMessage = signatureError();
    if (errorMessage) { fieldError("signature-error", errorMessage, "signature"); return; }
    const revision = state.requestRevision;
    notify("");
    setBusy(true);
    fieldError("signature-error", "", "signature");
    try {
      const job = await api("/api/builds", { method: "POST", body: { challenge_id: state.challenge.challenge_id, signature: $("signature").value.trim() } });
      if (revision !== state.requestRevision) return;
      state.proofVerified = true;
      clearTimeout(state.expiryTimer);
      $("challenge-panel").hidden = true;
      $("signature").value = "";
      acceptJob(job);
    } catch (error) {
      if (revision === state.requestRevision) {
        fieldError("signature-error", error.message, "signature");
        if ([404, 409, 410].includes(error.status)) {
          state.challengeExpired = true;
          state.challengeDeadline = performance.now();
          updateChallengeExpiry();
        }
      }
    }
    finally { setBusy(false); }
  }

  function stopPolling() {
    state.jobGeneration += 1;
    clearTimeout(state.jobTimer);
    state.jobController?.abort();
    state.jobController = null;
  }

  function acceptJob(job) {
    if (!job.job_id || typeof job.access_token !== "string" || !job.access_token) throw new Error("The server did not return a complete job access response.");
    stopPolling();
    state.job = { ...job, accessToken: job.access_token, requestPublicKey: $("public-key").value.trim() };
    state.jobRequestRevision = state.requestRevision;
    state.retryKeyRequired = false;
    renderJob();
    $("job-panel").hidden = false;
    $("job-heading").focus({ preventScroll: true });
    $("job-panel").scrollIntoView({ block: "nearest", behavior: "smooth" });
    pollJob();
  }

  function renderJob() {
    const job = state.job;
    if (!job) return;
    const titles = { queued: "Build queued", building: "Building your system", succeeded: "Your build is ready", failed: "Build failed" };
    const details = {
      queued: "Waiting for an available build worker.",
      building: "The worker is building your selected configuration.",
      succeeded: "Your artifact is ready. Its SHA-256 is shown below.",
      failed: "This request stopped without a completed artifact. Review the error before trying again.",
    };
    $("job-heading").textContent = titles[job.status] || "Build status";
    $("job-id").textContent = `REQUEST ${job.job_id}`;
    $("job-configuration").textContent = `${job.manifest?.profile === "dev" ? "Development" : "Release"} · Config ${job.manifest?.config_hash?.slice(0, 12) || "unavailable"}`;
    const apps = job.manifest?.buildins;
    const appSummary = Array.isArray(apps)
      ? apps.length ? `${apps.length} selected — ${apps.map(buildinLabelFor).join(", ")}` : "None · Kernel only"
      : Array.isArray(job.manifest?.features) ? job.manifest.features.includes("buildins") ? "Included · individual app list unavailable" : "None · Kernel only" : "Unavailable";
    $("job-buildins").textContent = `Bundled apps: ${appSummary}`;
    const sizes = Array.isArray(apps) ? sizeMetrics(apps) : null;
    $("job-size-estimate").hidden = !sizes?.data || sizes.selectedBytes === null;
    $("job-size-estimate").textContent = sizes?.data && sizes.selectedBytes !== null
      ? `${sizes.data.reference?.label || "Measured snapshot"} ${referenceProfile(sizes.data).toLowerCase()} reference: ${formatKilobytes(sizes.selectedBytes)} in selected app packages${sizes.omittedBytes !== null ? ` · ${formatKilobytes(sizes.omittedBytes)} of app payload omitted` : ""}. Rebuilt sizes may vary.`
      : "";
    $("job-features").replaceChildren();
    for (const id of job.manifest?.features || []) {
      if (id !== "buildins") $("job-features").append(make("span", "feature-chip", labelFor(id)));
    }
    const phases = {
      dispatching: "Submitting your configuration to the builder.",
      "waiting-for-runner": "Waiting for an available build machine.",
      compiling: "Building your selected configuration.",
      collecting: "Checking the completed build and preparing your download.",
    };
    $("job-detail").textContent = (job.status === "building" && phases[job.phase]) || details[job.status] || `Server status: ${job.status}`;
    $("job-panel").className = `job-card${job.status === "succeeded" ? " finished" : job.status === "failed" ? " failed" : ""}`;
    const progress = ["queued", "building", "succeeded"].indexOf(job.status);
    for (const step of $("job-progress").children) {
      const index = ["queued", "building", "succeeded"].indexOf(step.dataset.stage);
      step.classList.toggle("reached", index <= progress);
      step.classList.toggle("current", index === progress && job.status !== "succeeded");
      if (index === progress && job.status !== "succeeded") step.setAttribute("aria-current", "step"); else step.removeAttribute("aria-current");
    }
    fieldError("job-error", typeof job.error === "string" ? job.error : "");
    const ready = job.status === "succeeded" && Boolean(job.artifact);
    $("artifact-meta").hidden = !ready;
    $("download-artifact").hidden = !ready;
    $("download-receipt").hidden = !(job.status === "succeeded" && job.receipt);
    $("retry-job").hidden = job.status !== "failed";
    if (ready) {
      $("artifact-name").textContent = job.artifact.filename || "TRUEOS artifact";
      $("artifact-sha").textContent = job.artifact.sha256 || "Digest unavailable";
      $("artifact-size").textContent = formatSize(job.artifact.size);
      $("artifact-size").title = Number.isFinite(job.artifact.size) ? `${job.artifact.size.toLocaleString()} bytes` : "";
    }
    updateActions();
  }

  function saveAccess() {
    if (!state.job) return;
    const ticket = { schema: "trueos-job-access/v1", origin: location.origin, job_id: state.job.job_id, access_token: state.job.accessToken };
    download(JSON.stringify(ticket, null, 2) + "\n", `trueos-build-access-${state.job.job_id.slice(0, 12)}.json`);
    notify("Private build access file saved. Keep it private: it grants access to this build. Use ‘Resume a saved build’ to reopen the request.");
  }

  async function resumeBuild(file) {
    if (!file || state.busy) return;
    fieldError("resume-error", "");
    if (file.size > 4096) { fieldError("resume-error", "The access file exceeds the 4 KB limit."); $("resume-file").value = ""; return; }
    setBusy(true);
    try {
      const raw = await file.text();
      if (privateKeyLike(raw)) throw new Error("That is a private key and has not been sent. Choose a saved build access JSON file.");
      let ticket;
      try { ticket = JSON.parse(raw); } catch { throw new Error("Choose a valid build access JSON file."); }
      if (!ticket || Array.isArray(ticket) || Object.keys(ticket).sort().join(",") !== "access_token,job_id,origin,schema" || ticket.schema !== "trueos-job-access/v1") throw new Error("This is not a supported TRUEOS build access file.");
      if (ticket.origin !== location.origin) throw new Error("This access file belongs to a different server. Open it on its original server; no credentials were sent.");
      if (typeof ticket.job_id !== "string" || !/^[0-9a-f]{32}$/.test(ticket.job_id) || typeof ticket.access_token !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(ticket.access_token)) throw new Error("The build ID or access token has an invalid format; no credentials were sent.");
      const job = await api(`/api/jobs/${encodeURIComponent(ticket.job_id)}`, { token: ticket.access_token });
      if (job.job_id !== ticket.job_id || !Array.isArray(job.manifest?.features) || !["queued", "building", "succeeded", "failed"].includes(job.status)) throw new Error("The server did not return a valid build record.");
      stopPolling();
      state.job = { ...job, accessToken: ticket.access_token };
      state.jobRequestRevision = -1;
      renderJob();
      $("job-panel").hidden = false;
      $("job-heading").focus({ preventScroll: true });
      $("job-panel").scrollIntoView({ block: "nearest", behavior: "smooth" });
      notify("Build resumed. Its original configuration is shown with the request; the selector above remains editable.");
      if (["queued", "building"].includes(job.status)) pollJob();
    } catch (error) { fieldError("resume-error", error.message); }
    finally { $("resume-file").value = ""; setBusy(false); }
  }

  function retryJob() {
    const job = state.job;
    if (!job || job.status !== "failed" || !job.manifest || state.busy) return;
    const known = new Set(kernelFeatures().map((feature) => feature.id));
    state.selected = new Set(job.manifest.features.filter((id) => known.has(id)));
    const knownBuildins = new Set(state.catalog.buildins.map(({ id }) => id));
    const recordedBuildins = Array.isArray(job.manifest.buildins) ? job.manifest.buildins : null;
    const unavailableBuildins = recordedBuildins?.filter((id) => !knownBuildins.has(id)) || [];
    state.selectedBuildins = new Set(recordedBuildins
      ? recordedBuildins.filter((id) => knownBuildins.has(id))
      : job.manifest.features.includes("buildins") ? knownBuildins : []);
    state.activePreset = null;
    state.basePreset = state.catalog.default_preset || state.catalog.presets[0].id;
    state.profile = job.manifest.profile === "dev" ? "dev" : "release";
    for (const input of document.querySelectorAll("input[name=profile]")) input.checked = input.value === state.profile;
    const keyed = Boolean(job.key_fingerprint || job.requestPublicKey);
    $("public-key").value = keyed ? job.requestPublicKey || "" : "";
    fieldError("key-error", "", "public-key");
    configurationChanged();
    state.retryKeyRequired = keyed;
    updateActions();
    let message = "The failed build’s configuration is restored. Review it and create a new request.";
    if (!recordedBuildins && job.manifest.features.includes("buildins")) message += " The older manifest did not record individual apps; the current all-app set is selected for review.";
    if (unavailableBuildins.length) message += ` These apps are no longer in the current catalog and could not be restored: ${unavailableBuildins.join(", ")}.`;
    if (keyed) message += " A fresh public-key challenge is required.";
    if (job.manifest.source_commit !== state.sourceCommit) message += " The server now targets a different source revision; the new manifest will show that revision.";
    notify(message);
    $(keyed ? "public-key" : "build-button").focus({ preventScroll: true });
    $("identity-heading").scrollIntoView({ block: "start", behavior: "smooth" });
  }

  function formatSize(bytes) {
    if (!Number.isFinite(bytes) || bytes < 0) return "Size unavailable";
    if (bytes < 1024) return `${bytes} B`;
    const units = ["KiB", "MiB", "GiB", "TiB"];
    let value = bytes / 1024;
    let index = 0;
    while (value >= 1024 && index < units.length - 1) { value /= 1024; index += 1; }
    return `${value.toLocaleString(undefined, { maximumFractionDigits: 2 })} ${units[index]}`;
  }

  async function pollJob() {
    if (!state.job) return;
    clearTimeout(state.jobTimer);
    state.jobController?.abort();
    const generation = state.jobGeneration;
    const { job_id: id, accessToken: token } = state.job;
    const controller = new AbortController();
    state.jobController = controller;
    $("refresh-job").hidden = true;
    try {
      const job = await api(`/api/jobs/${encodeURIComponent(id)}`, { token, signal: controller.signal });
      if (generation !== state.jobGeneration || controller !== state.jobController) return;
      if (!["queued", "building", "succeeded", "failed"].includes(job.status)) throw new Error("The worker returned an unknown status. Refresh to check again.");
      state.job = { ...state.job, ...job, accessToken: token };
      renderJob();
      if (["queued", "building"].includes(job.status)) state.jobTimer = setTimeout(pollJob, 2500);
    } catch (error) {
      if (error.name === "AbortError" || generation !== state.jobGeneration || controller !== state.jobController) return;
      fieldError("job-error", `Status updates paused: ${error.message}`);
      $("refresh-job").hidden = false;
    }
  }

  async function downloadArtifact() {
    const job = state.job;
    if (!job || job.status !== "succeeded" || !job.artifact) return;
    $("download-artifact").disabled = true;
    $("download-artifact").textContent = "Downloading…";
    try {
      const response = await fetch(apiURL(`/api/jobs/${encodeURIComponent(job.job_id)}/artifact`), {
        headers: { Authorization: `Bearer ${job.accessToken}` }, mode: "same-origin", credentials: "omit", cache: "no-store",
      });
      if (!response.ok) {
        let message = `Download failed (${response.status}).`;
        try { const data = await response.json(); if (typeof data.error === "string") message = data.error; } catch { /* Keep the HTTP error. */ }
        throw new Error(message);
      }
      download(await response.blob(), job.artifact.filename || `TRUEOS-${job.job_id.slice(0, 12)}.tar.lz4`);
    } catch (error) { notify(error.message, true); }
    finally {
      $("download-artifact").disabled = false;
      $("download-artifact").textContent = "Download build ↓";
    }
  }

  function wireEvents() {
    $("buildins-all").addEventListener("click", () => selectBuildins(true));
    $("buildins-none").addEventListener("click", () => selectBuildins(false));
    for (const input of document.querySelectorAll("input[name=profile]")) input.addEventListener("change", () => {
      if (!input.checked || state.busy) return;
      state.profile = input.value;
      configurationChanged();
    });
    $("public-key").addEventListener("input", () => {
      if (state.busy) return;
      clearRequest();
      if (privateKeyLike($("public-key").value)) {
        $("public-key").value = "";
        fieldError("key-error", "That looks like a private key. It was cleared and has not been sent. Paste the public .pub key instead.", "public-key");
      } else fieldError("key-error", "", "public-key");
      updateActions();
    });
    $("public-key").addEventListener("blur", () => {
      if ($("public-key").value.trim()) fieldError("key-error", keyError(), "public-key");
    });
    $("signature").addEventListener("input", () => {
      if (privateKeyLike($("signature").value)) {
        $("signature").value = "";
        fieldError("signature-error", "That looks like a private key. It was cleared and has not been sent. Use the generated .sig file.", "signature");
      } else fieldError("signature-error", "", "signature");
      $("signature-file").value = "";
      $("signature-file-name").textContent = "Signature entered manually.";
      updateActions();
    });
    $("signature").addEventListener("blur", () => {
      if ($("signature").value.trim()) fieldError("signature-error", signatureError(), "signature");
    });
    $("signature-file").addEventListener("change", async () => {
      const file = $("signature-file").files[0];
      if (!file) return;
      const revision = state.requestRevision;
      if (file.size > 16384) { fieldError("signature-error", "The signature file exceeds the 16 KB limit.", "signature"); $("signature-file").value = ""; return; }
      try {
        const content = await file.text();
        if (revision !== state.requestRevision) return;
        if (privateKeyLike(content)) {
          $("signature").value = "";
          $("signature-file").value = "";
          $("signature-file-name").textContent = "Private key rejected locally.";
          fieldError("signature-error", "That is a private key and has not been sent. Choose the generated .sig file.", "signature");
        } else {
          $("signature").value = content;
          $("signature-file-name").textContent = file.name;
          fieldError("signature-error", signatureError(), "signature");
        }
      } catch { fieldError("signature-error", "The signature file could not be read.", "signature"); }
      updateActions();
    });
    $("build-button").addEventListener("click", startRequest);
    $("verify-button").addEventListener("click", verifyAndQueue);
    $("download-challenge").addEventListener("click", () => {
      if (state.challenge && !state.challengeExpired) download(state.challenge.challenge, "trueos-challenge.json");
    });
    $("download-manifest").addEventListener("click", () => {
      if (state.manifest) download(JSON.stringify(state.manifest, null, 2) + "\n", "trueos-build-manifest.json");
    });
    $("copy-command").addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText($("sign-command").textContent);
        $("copy-command").textContent = "Copied";
        setTimeout(() => { $("copy-command").textContent = "Copy"; }, 2000);
      } catch { notify("Clipboard access is unavailable. Select the signing command to copy it."); }
    });
    $("refresh-job").addEventListener("click", pollJob);
    $("save-access").addEventListener("click", saveAccess);
    $("resume-file").addEventListener("change", () => resumeBuild($("resume-file").files[0]));
    $("retry-job").addEventListener("click", retryJob);
    $("download-artifact").addEventListener("click", downloadArtifact);
    $("download-receipt").addEventListener("click", () => {
      if (state.job?.receipt) download(JSON.stringify(state.job.receipt, null, 2) + "\n", `trueos-license-${state.job.job_id.slice(0, 12)}.json`);
    });
    window.addEventListener("pagehide", () => { stopPolling(); state.planController?.abort(); clearTimeout(state.expiryTimer); });
    window.addEventListener("pageshow", (event) => {
      if (!event.persisted) return;
      if (state.job) pollJob();
      if (state.catalog && !state.manifest) schedulePlan(true);
      if (state.challenge && !state.proofVerified) updateChallengeExpiry();
    });
  }

  async function initialize() {
    wireEvents();
    for (const input of document.querySelectorAll("input[name=profile]")) input.disabled = true;
    try {
      const data = await api("/api/catalog");
      if (!Array.isArray(data.catalog?.features) || !data.catalog.features.length || !Array.isArray(data.catalog?.presets) || !data.catalog.presets.length || !Array.isArray(data.catalog?.buildins)) throw new Error("The server did not provide a usable feature and app catalog.");
      state.catalog = data.catalog;
      state.capabilities = { anonymous: data.capabilities?.anonymous === true, licensed: data.capabilities?.licensed === true };
      state.sourceCommit = data.source_commit || "";
      // The command uses only the fixed protocol namespace, never arbitrary server text.
      if (data.namespace && data.namespace !== state.namespace) throw new Error("This server uses an unsupported signing namespace.");
      const defaultPreset = state.catalog.presets.find((preset) => preset.id === state.catalog.default_preset)
        || state.catalog.presets.find((preset) => preset.id === "universal") || state.catalog.presets[0];
      applyPreset(defaultPreset);
      $("source-commit").textContent = state.sourceCommit.slice(0, 12) || "Not configured";
      $("source-commit").title = state.sourceCommit;
      if (/^[0-9a-f]{40}$/i.test(state.sourceCommit)) $("source-commit").href = `https://github.com/t4ce/TRUEOS/commit/${state.sourceCommit}`;
      $("connection-status").classList.add("connected");
      $("connection-label").textContent = "Service connected";
      $("worker-state").textContent = data.capabilities?.worker_mode === "github-actions" ? "Builds run on GitHub Actions" : "Build service connected";
      $("artifact-visibility").hidden = data.capabilities?.artifacts_public !== true;
      renderCatalog();
      renderSelection();
      for (const input of document.querySelectorAll("input[name=profile]")) input.disabled = false;
      schedulePlan(true);
    } catch (error) {
      $("connection-status").classList.add("failed");
      $("connection-label").textContent = "Service unavailable";
      $("presets").replaceChildren(make("p", "loading-copy", "Presets are unavailable until the build service connects."));
      $("features").replaceChildren(make("p", "loading-copy", "Reload this page when the service is available."));
      $("buildins").replaceChildren(make("p", "loading-copy", "Bundled app choices are unavailable until the build service connects."));
      $("worker-state").textContent = "Build service unavailable";
      $("manifest-json").textContent = "No configuration available.";
      planStatus("OFFLINE", "invalid");
      notify(error.message, true);
      updateActions();
    }
  }

  initialize();
})();
