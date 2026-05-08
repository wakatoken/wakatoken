const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open } = window.__TAURI__.shell;

const deviceAuthBtn = document.getElementById("device-auth-btn");
const deviceAuthResult = document.getElementById("device-auth-result");
const dashboardLink = document.getElementById("dashboard-link");
const docsLink = document.getElementById("docs-link");
const statInput = document.getElementById("stat-input");
const statOutput = document.getElementById("stat-output");
const statTotal = document.getElementById("stat-total");
const statsFooter = document.getElementById("stats-footer");
let baseUrl = "";
const baseUrlReady = invoke("get_base_url").then(url => {
  baseUrl = url;
  return url;
});

async function loadConfig() {
  const config = await invoke("get_config");
  if (config.access_token) {
    deviceAuthResult.textContent = "Signed in";
    deviceAuthResult.className = "test-result success";
  } else {
    deviceAuthResult.textContent = "";
    deviceAuthResult.className = "test-result";
  }
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

deviceAuthBtn.addEventListener("click", async () => {
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
    deviceAuthBtn.textContent = "Sign in with Browser";
    deviceAuthBtn.disabled = false;
  }
});

async function absoluteUrl(url) {
  if (url.startsWith("http://") || url.startsWith("https://")) return url;
  return `${await baseUrlReady}${url}`;
}

// External links — URL derived from Rust's single BASE_URL constant
dashboardLink.addEventListener("click", async (e) => {
  e.preventDefault();
  await baseUrlReady;
  open(`${baseUrl}/dashboard`);
});

docsLink.addEventListener("click", async (e) => {
  e.preventDefault();
  await baseUrlReady;
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
