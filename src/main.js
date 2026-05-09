const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open } = window.__TAURI__.shell;

const deviceAuthBtn = document.getElementById("device-auth-btn");
const deviceAuthResult = document.getElementById("device-auth-result");
const statInput = document.getElementById("stat-input");
const statOutput = document.getElementById("stat-output");
const statTotal = document.getElementById("stat-total");
const statsFooter = document.getElementById("stats-footer");
const workspaceKicker = document.getElementById("workspace-kicker");
const workspaceTitle = document.getElementById("workspace-title");
const localTotal = document.getElementById("local-total");
const localInput = document.getElementById("local-input");
const localOutput = document.getElementById("local-output");
const localSessions = document.getElementById("local-sessions");
const todayLocalTotal = document.getElementById("today-local-total");
const todayLocalInput = document.getElementById("today-local-input");
const todayLocalOutput = document.getElementById("today-local-output");
const todayLocalSessions = document.getElementById("today-local-sessions");
const runtimeList = document.getElementById("runtime-list");
const runtimeFilter = document.getElementById("runtime-filter");
const sessionList = document.getElementById("session-list");
const rescanButtons = document.querySelectorAll(".rescan-btn");
const syncNowButtons = document.querySelectorAll(".sync-now-btn");
const settingsBtn = document.getElementById("settings-btn");
const settingsModal = document.getElementById("settings-modal");
const settingsCloseBtn = document.getElementById("settings-close-btn");
const accountArea = document.getElementById("account-area");
const modalAccount = document.getElementById("modal-account");
const runtimeSettings = document.querySelector(".runtime-settings");
const menuItems = document.querySelectorAll(".menu-item");
const viewSections = document.querySelectorAll("[data-view-section]");
const viewPanels = document.querySelectorAll("[data-view-panel]");
const settingsTabs = document.querySelectorAll(".modal-tab");
const settingsPages = document.querySelectorAll(".modal-page");
let baseUrl = "";
const baseUrlReady = invoke("get_base_url").then(url => {
  baseUrl = url;
  return url;
});
let selectedRuntime = "all";
let appConfig = null;
let account = { signedIn: false, name: "", email: "", image: null };

async function loadConfig() {
  appConfig = await invoke("get_config");
  syncRuntimeSettings(appConfig.enabled_runtimes || []);
  if (appConfig.access_token) {
    deviceAuthBtn.textContent = "Sign out";
    deviceAuthResult.textContent = "Signed in";
    deviceAuthResult.className = "test-result success";
  } else {
    deviceAuthBtn.textContent = "Sign in with Browser";
    deviceAuthResult.textContent = "";
    deviceAuthResult.className = "test-result";
  }
}

async function loadAccount() {
  account = await invoke("get_account");
  renderAccount();
}

async function loadStatus() {
  const s = await invoke("get_sync_status");
  renderStatus(s);
}

async function loadLocalDashboard() {
  const dashboard = await invoke("get_local_dashboard");
  const sessions = await invoke("list_sessions", {
    runtime: selectedRuntime === "all" ? null : selectedRuntime,
  });
  renderLocalDashboard(dashboard);
  renderRuntimeFilter(dashboard.runtimes);
  renderRuntimes(dashboard.runtimes, dashboard.totalTokens);
  renderSessions(sessions);
}

function renderStatus(s) {
  statInput.textContent = formatTokensShort(s.todayInputTokens);
  statOutput.textContent = formatTokensShort(s.todayOutputTokens);
  statTotal.textContent = formatCount(s.totalSynced);

  if (s.lastSyncTs === 0) {
    statsFooter.textContent = "Not synced yet";
  } else if (s.lastSyncOk) {
    statsFooter.textContent = `Last sync: ${formatTimeAgo(s.lastSyncTs)}`;
  } else {
    statsFooter.textContent = `Sync failed: ${s.lastError}`;
  }
}

function renderLocalDashboard(dashboard) {
  localTotal.textContent = formatTokensShort(dashboard.totalTokens);
  localInput.textContent = formatTokensShort(dashboard.totalInputTokens);
  localOutput.textContent = formatTokensShort(dashboard.totalOutputTokens);
  localSessions.textContent = formatCount(dashboard.sessionCount);
  todayLocalTotal.textContent = formatTokensShort(dashboard.todayTokens);
  todayLocalInput.textContent = formatTokensShort(dashboard.todayInputTokens);
  todayLocalOutput.textContent = formatTokensShort(dashboard.todayOutputTokens);
  todayLocalSessions.textContent = formatCount(dashboard.todaySessionCount);
}

function renderRuntimeFilter(runtimes) {
  const existing = new Set(
    [...runtimeFilter.options].map(option => option.value),
  );
  for (const runtime of runtimes) {
    if (existing.has(runtime.runtime)) continue;
    const option = document.createElement("option");
    option.value = runtime.runtime;
    option.textContent = runtimeLabel(runtime.runtime);
    runtimeFilter.appendChild(option);
  }
  runtimeFilter.value = selectedRuntime;
}

function renderRuntimes(runtimes, totalTokens) {
  if (!runtimes.length) {
    runtimeList.innerHTML = `<div class="empty-state">No local runtime data yet</div>`;
    return;
  }

  runtimeList.innerHTML = runtimes
    .sort((a, b) => b.totalTokens - a.totalTokens)
    .map(runtime => {
      const share = totalTokens > 0 ? Math.round((runtime.totalTokens / totalTokens) * 100) : 0;
      return `
      <button class="runtime-row" data-runtime="${escapeHtml(runtime.runtime)}">
        <span class="runtime-main">
          <strong>${runtimeLabel(runtime.runtime)}</strong>
          <small>${formatCount(runtime.sessionCount)} sessions · last seen ${formatTime(runtime.lastSeenAt)}</small>
          <span class="runtime-bar"><span style="width: ${share}%"></span></span>
        </span>
        <span class="runtime-metrics">
          <b>${formatTokensShort(runtime.totalTokens)}</b>
          <small>${formatTokensShort(runtime.inputTokens)} in · ${formatTokensShort(runtime.outputTokens)} out</small>
          <small>${formatTokensShort(runtime.cacheReadTokens + runtime.cacheWriteTokens)} cache</small>
          <small>${share}% of total</small>
        </span>
      </button>
    `;
    })
    .join("");

  for (const row of runtimeList.querySelectorAll(".runtime-row")) {
    row.addEventListener("click", () => {
      selectedRuntime = row.dataset.runtime;
      runtimeFilter.value = selectedRuntime;
      loadLocalDashboard();
      showView("sessions");
    });
  }
}

function renderSessions(sessions) {
  if (!sessions.length) {
    sessionList.innerHTML = `<div class="empty-state">No sessions match this filter</div>`;
    return;
  }

  sessionList.innerHTML = sessions.map(session => `
    <article class="session-row">
      <div class="session-main">
        <strong>${escapeHtml(session.project || "Unknown project")}</strong>
        <small>${runtimeLabel(session.runtime)} · ${escapeHtml(session.model || "Unknown model")}</small>
      </div>
      <div class="session-meta">
        <span>${formatTokensShort(session.totalTokens)}</span>
        <span>${formatCount(session.eventCount)} events</span>
        <span class="status ${session.status}">${escapeHtml(session.status)}</span>
      </div>
      <div class="session-path">${escapeHtml(session.path)}</div>
    </article>
  `).join("");
}

function renderAccount() {
  if (!account.signedIn) {
    accountArea.innerHTML = `<button class="login-inline" type="button">Sign in</button>`;
    modalAccount.innerHTML = `<p class="account-muted">Not signed in.</p>`;
    accountArea.querySelector(".login-inline").addEventListener("click", openSettings);
    return;
  }

  const avatar = account.image
    ? `<img src="${escapeHtml(account.image)}" alt="">`
    : `<span>${escapeHtml(accountInitial(account.name || account.email))}</span>`;
  accountArea.innerHTML = `
    <div class="avatar">${avatar}</div>
    <div class="account-copy">
      <strong>${escapeHtml(account.name || "Signed in")}</strong>
      <small>${escapeHtml(account.email || "")}</small>
    </div>
  `;
  modalAccount.innerHTML = `
    <div class="account-card">
      <div class="avatar large">${avatar}</div>
      <div>
        <strong>${escapeHtml(account.name || "Signed in")}</strong>
        <small>${escapeHtml(account.email || "")}</small>
      </div>
    </div>
  `;
}

function accountInitial(value) {
  return (value || "W").trim().charAt(0).toUpperCase();
}

function syncRuntimeSettings(enabled) {
  const enabledSet = new Set(enabled);
  for (const input of runtimeSettings.querySelectorAll("input")) {
    input.checked = enabledSet.has(input.value);
  }
}

async function saveRuntimeSettings() {
  const enabled = [...runtimeSettings.querySelectorAll("input:checked")]
    .map(input => input.value);
  appConfig = await invoke("save_runtime_settings", { enabledRuntimes: enabled });
  syncRuntimeSettings(appConfig.enabled_runtimes || []);
  await loadLocalDashboard();
}

function openSettings() {
  showSettingsTab("account");
  settingsModal.classList.remove("hidden");
}

function closeSettings() {
  settingsModal.classList.add("hidden");
}

function showSettingsTab(tab) {
  for (const button of settingsTabs) {
    button.classList.toggle("active", button.dataset.settingsTab === tab);
  }
  for (const page of settingsPages) {
    page.classList.toggle("hidden", page.dataset.settingsPage !== tab);
  }
}

function showView(view) {
  for (const button of menuItems) {
    button.classList.toggle("active", button.dataset.view === view);
  }

  const showDashboard = view === "dashboard";
  document.querySelector(".overview-grid").classList.toggle("view-hidden", !showDashboard);
  document.querySelector(".content-grid").classList.add("single-view");
  document.querySelector(".content-grid").classList.toggle("fill-view", !showDashboard);

  for (const section of viewSections) {
    if (section.dataset.viewSection === "detail") continue;
    section.classList.toggle("view-hidden", !showDashboard);
  }
  for (const panel of viewPanels) {
    panel.classList.toggle("view-hidden", panel.dataset.viewPanel !== view);
  }

  const titles = {
    dashboard: ["Local Analytics", "Dashboard"],
    sessions: ["Local Sessions", "Sessions"],
  };
  const [kicker, title] = titles[view];
  workspaceKicker.textContent = kicker;
  workspaceTitle.textContent = title;
}

function formatTokensShort(n) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return `${n}`;
}

function formatCount(n) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return `${n}`;
}

function formatTimeAgo(ts) {
  const diff = Math.floor(Date.now() / 1000) - ts;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function formatTime(ms) {
  if (!ms) return "never";
  return new Date(ms).toLocaleDateString();
}

function runtimeLabel(runtime) {
  const labels = {
    "claude-code": "Claude Code",
    "codex-cli": "Codex CLI",
    "copilot-agent": "Copilot Agent",
    "gemini-cli": "Gemini CLI",
  };
  return labels[runtime] || runtime;
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

deviceAuthBtn.addEventListener("click", async () => {
  if (account.signedIn) {
    deviceAuthBtn.disabled = true;
    deviceAuthBtn.textContent = "Signing out...";
    try {
      appConfig = await invoke("sign_out");
      await loadAccount();
      await loadConfig();
    } catch (e) {
      deviceAuthResult.textContent = e;
      deviceAuthResult.className = "test-result error";
    } finally {
      deviceAuthBtn.disabled = false;
    }
    return;
  }

  deviceAuthBtn.disabled = true;
  deviceAuthBtn.textContent = "Starting...";
  deviceAuthResult.textContent = "";
  try {
    const data = await invoke("start_device_auth");
    deviceAuthResult.textContent = `Enter code ${data.userCode}`;
    deviceAuthResult.className = "test-result";
    const verificationUri = data.verificationUriComplete || data.verificationUri;
    await open(await absoluteUrl(verificationUri));
    deviceAuthBtn.textContent = "Waiting...";

    const deadline = Date.now() + data.expiresIn * 1000;
    const interval = Math.max(data.interval, 1) * 1000;
    while (Date.now() < deadline) {
      await new Promise(resolve => setTimeout(resolve, interval));
      if (await invoke("poll_device_auth", { deviceCode: data.deviceCode })) {
        await loadConfig();
        await loadAccount();
        deviceAuthResult.textContent = "Signed in";
        deviceAuthResult.className = "test-result success";
        return;
      }
    }

    deviceAuthResult.textContent = "Device code expired";
    deviceAuthResult.className = "test-result error";
  } catch (e) {
    deviceAuthResult.textContent = e;
    deviceAuthResult.className = "test-result error";
  } finally {
    deviceAuthBtn.textContent = account.signedIn ? "Sign out" : "Sign in with Browser";
    deviceAuthBtn.disabled = false;
  }
});

async function absoluteUrl(url) {
  if (url.startsWith("http://") || url.startsWith("https://")) return url;
  return `${await baseUrlReady}${url}`;
}

// Listen for sync progress events (from background sync)
listen("sync-progress", (event) => {
  const { phase, detail } = event.payload;
  if (phase === "done" || phase === "error") {
    loadStatus();
    loadLocalDashboard();
  } else {
    statsFooter.textContent = detail;
  }
});

runtimeFilter.addEventListener("change", () => {
  selectedRuntime = runtimeFilter.value;
  loadLocalDashboard();
});

async function rescanLocalData() {
  setButtonsBusy(rescanButtons, true, "Scanning...");
  try {
    await invoke("rescan_local_stats");
    await loadLocalDashboard();
  } catch (e) {
    statsFooter.textContent = `Rescan failed: ${e}`;
  } finally {
    setButtonsBusy(rescanButtons, false, "Rescan");
  }
}

async function syncNow() {
  setButtonsBusy(syncNowButtons, true, "Syncing...");
  try {
    await invoke("sync_now");
    await loadStatus();
    await loadLocalDashboard();
  } catch (e) {
    statsFooter.textContent = `Sync failed: ${e}`;
  } finally {
    setButtonsBusy(syncNowButtons, false, "Sync now");
  }
}

function setButtonsBusy(buttons, busy, text) {
  for (const button of buttons) {
    button.disabled = busy;
    button.textContent = text;
  }
}

for (const button of rescanButtons) {
  button.addEventListener("click", rescanLocalData);
}

for (const button of syncNowButtons) {
  button.addEventListener("click", syncNow);
}

settingsBtn.addEventListener("click", openSettings);
settingsCloseBtn.addEventListener("click", closeSettings);
settingsModal.addEventListener("click", (event) => {
  if (event.target === settingsModal) closeSettings();
});

for (const tab of settingsTabs) {
  tab.addEventListener("click", () => showSettingsTab(tab.dataset.settingsTab));
}

runtimeSettings.addEventListener("change", () => {
  saveRuntimeSettings().catch((e) => {
    statsFooter.textContent = `Settings failed: ${e}`;
  });
});

for (const item of menuItems) {
  item.addEventListener("click", () => {
    showView(item.dataset.view);
  });
}

showView("dashboard");
loadConfig();
loadAccount();
loadStatus();
loadLocalDashboard();

// Refresh status every 30s while the window is open
setInterval(() => {
  loadStatus();
  loadLocalDashboard();
}, 30_000);
