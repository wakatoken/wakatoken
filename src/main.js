const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open } = window.__TAURI__.shell;

const apiKeyInput = document.getElementById("api-key");
const saveBtn = document.getElementById("save-btn");
const testBtn = document.getElementById("test-btn");
const toggleKeyBtn = document.getElementById("toggle-key");
const dashboardLink = document.getElementById("dashboard-link");
const docsLink = document.getElementById("docs-link");
const statInput = document.getElementById("stat-input");
const statOutput = document.getElementById("stat-output");
const statTotal = document.getElementById("stat-total");
const statsFooter = document.getElementById("stats-footer");
const testResult = document.getElementById("test-result");

async function loadConfig() {
  const config = await invoke("get_config");
  apiKeyInput.value = config.api_key || "";
}

async function loadStatus() {
  const s = await invoke("get_sync_status");
  renderStatus(s);
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

// Save config
saveBtn.addEventListener("click", async () => {
  saveBtn.textContent = "Saving...";
  await invoke("save_config", { apiKey: apiKeyInput.value.trim() });
  saveBtn.textContent = "Saved!";
  setTimeout(() => { saveBtn.textContent = "Save"; }, 1500);
});

// Test API key connection
testBtn.addEventListener("click", async () => {
  const key = apiKeyInput.value.trim();
  if (!key) {
    testResult.textContent = "Please enter an API key first";
    testResult.className = "test-result error";
    return;
  }
  testBtn.textContent = "Testing...";
  testBtn.disabled = true;
  testResult.textContent = "";
  try {
    const msg = await invoke("test_api_key", { apiKey: key });
    testResult.textContent = msg;
    testResult.className = "test-result success";
  } catch (e) {
    testResult.textContent = e;
    testResult.className = "test-result error";
  }
  testBtn.textContent = "Test Connection";
  testBtn.disabled = false;
});

// Toggle API key visibility
toggleKeyBtn.addEventListener("click", () => {
  apiKeyInput.type = apiKeyInput.type === "password" ? "text" : "password";
});

// External links — URL derived from Rust's single BASE_URL constant
let baseUrl = "";
invoke("get_base_url").then(url => { baseUrl = url; });

dashboardLink.addEventListener("click", (e) => {
  e.preventDefault();
  open(`${baseUrl}/dashboard`);
});

docsLink.addEventListener("click", (e) => {
  e.preventDefault();
  open(`${baseUrl}/docs`);
});

// Listen for sync progress events (from background sync)
listen("sync-progress", (event) => {
  const { phase, detail } = event.payload;
  if (phase === "done" || phase === "error") {
    loadStatus();
  } else {
    statsFooter.textContent = detail;
  }
});

loadConfig();
loadStatus();

// Refresh status every 30s while the window is open
setInterval(loadStatus, 30_000);
