const ENDPOINT = "http://127.0.0.1:16800";
const dot = document.getElementById("dot");
const statusEl = document.getElementById("status");
const launchBtn = document.getElementById("launchBtn");
const toggleEl = document.getElementById("toggle");
const sniffToggleEl = document.getElementById("sniffToggle");
const urlInput = document.getElementById("urlInput");
const sendBtn = document.getElementById("sendBtn");
const resCount = document.getElementById("resCount");
const sniffSummary = document.getElementById("sniffSummary");
const openSniffBtn = document.getElementById("openSniffBtn");

chrome.storage.local.get({ enabled: true, sniffEnabled: false }, (res) => {
  toggleEl.checked = Boolean(res.enabled);
  const sniffing = Boolean(res.sniffEnabled);
  sniffToggleEl.checked = sniffing;
  if (sniffing) {
    loadPageSummary();
  } else {
    renderSniffDisabledState();
  }
});

toggleEl.addEventListener("change", () => {
  chrome.storage.local.set({ enabled: toggleEl.checked });
});

sniffToggleEl.addEventListener("change", () => {
  const on = sniffToggleEl.checked;
  chrome.storage.local.set({ sniffEnabled: on });
  if (on) {
    resCount.textContent = "扫描中...";
    loadPageSummary();
  } else {
    renderSniffDisabledState();
  }
});

function renderSniffDisabledState() {
  resCount.textContent = "已关闭";
  sniffSummary.innerHTML =
    '<span class="pill">嗅探默认关闭（仅下载时拉起客户端）；开启上方开关或点下方按钮可立即深度扫描当前页（含内嵌 iframe 与已加载流）</span>';
  openSniffBtn.textContent = "立即深度扫描并打开选择性下载窗";
}

async function checkStatus() {
  try {
    const headers = {};
    if (chrome.runtime && chrome.runtime.id) {
      headers["X-Siphon-Extension-Id"] = chrome.runtime.id;
    }
    const resp = await fetch(`${ENDPOINT}/api/ping`, { headers });
    const data = await resp.json();
    if (data && data.ok) {
      dot.classList.add("ok");
      statusEl.textContent = data.enabled ? "已连接" : "已在设置关闭";
      launchBtn.classList.add("hidden");
      return;
    }
  } catch (_) {}
  dot.classList.remove("ok");
  statusEl.textContent = "未启动";
  launchBtn.classList.remove("hidden");
}

launchBtn.addEventListener("click", () => {
  launchBtn.textContent = "正在启动...";
  launchBtn.disabled = true;
  chrome.runtime.sendMessage({ type: "LAUNCH_SIPHON" }, (resp) => {
    if (resp && resp.ok) {
      launchBtn.classList.add("hidden");
      checkStatus();
    } else {
      launchBtn.textContent = "重试启动";
      launchBtn.disabled = false;
    }
  });
});

function loadPageSummary() {
  chrome.runtime.sendMessage({ type: "SCAN_ACTIVE_TAB" }, (resp) => {
    if (chrome.runtime.lastError || !resp || !resp.ok) {
      resCount.textContent = "0 项";
      sniffSummary.innerHTML = '<span class="pill">当前页面暂无可扫描资源</span>';
      return;
    }
    const items = resp.items || [];
    resCount.textContent = `${items.length} 项`;
    if (items.length === 0) {
      sniffSummary.innerHTML = '<span class="pill">暂未捕获资源，播放音视频后将自动推送到选择性下载窗</span>';
      openSniffBtn.textContent = "打开 Siphon 选择性下载窗";
      return;
    }

    const counts = { hls: 0, video: 0, audio: 0, file: 0, p2p: 0, image: 0 };
    for (const it of items) {
      const k = (it.kind || "file").toLowerCase();
      if (k === "hls") counts.hls += 1;
      else if (k === "video") counts.video += 1;
      else if (k === "audio") counts.audio += 1;
      else if (k === "magnet" || k === "torrent" || k === "ed2k") counts.p2p += 1;
      else if (k === "image") counts.image += 1;
      else counts.file += 1;
    }

    sniffSummary.innerHTML = "";
    const addPill = (txt, cls = "") => {
      const span = document.createElement("span");
      span.className = `pill ${cls}`;
      span.textContent = txt;
      sniffSummary.appendChild(span);
    };
    if (counts.hls > 0) addPill(`HLS/DASH ${counts.hls}`, "hls");
    if (counts.video > 0) addPill(`视频 ${counts.video}`, "media");
    if (counts.audio > 0) addPill(`音频 ${counts.audio}`, "media");
    if (counts.file > 0) addPill(`文件 ${counts.file}`);
    if (counts.p2p > 0) addPill(`磁力/eD2k ${counts.p2p}`, "p2p");
    if (counts.image > 0) addPill(`图片 ${counts.image}`);

    openSniffBtn.textContent = `在 Siphon 选择性下载窗勾选 (${items.length} 项)`;
  });
}

openSniffBtn.addEventListener("click", () => {
  openSniffBtn.textContent = "正在打开 Siphon 选择性下载窗...";
  openSniffBtn.disabled = true;
  chrome.runtime.sendMessage({ type: "OPEN_SNIFF_WINDOW" }, (resp) => {
    if (resp && resp.ok) {
      window.close();
    } else {
      openSniffBtn.textContent = "打开失败，请确认已安装 Siphon";
      openSniffBtn.disabled = false;
    }
  });
});

sendBtn.addEventListener("click", () => {
  const url = urlInput.value.trim();
  const low = url.toLowerCase();
  if (
    !low.startsWith("http://") &&
    !low.startsWith("https://") &&
    !low.startsWith("magnet:?") &&
    !low.startsWith("ed2k://|file|")
  ) {
    return;
  }
  sendBtn.textContent = "正在发送到 Siphon...";
  sendBtn.disabled = true;
  chrome.runtime.sendMessage(
    { type: "SUBMIT_TO_SIPHON", url, referrer: "", silent: false },
    () => {
      urlInput.value = "";
      window.close();
    }
  );
});

checkStatus();
