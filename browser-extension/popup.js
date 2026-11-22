const ENDPOINT = "http://127.0.0.1:16800";
const dot = document.getElementById("dot");
const statusEl = document.getElementById("status");
const launchBtn = document.getElementById("launchBtn");
const toggleEl = document.getElementById("toggle");
const autoSniffEl = document.getElementById("autoSniffToggle");
const urlInput = document.getElementById("urlInput");
const sendBtn = document.getElementById("sendBtn");
const resCount = document.getElementById("resCount");
const sniffSummary = document.getElementById("sniffSummary");
const openSniffBtn = document.getElementById("openSniffBtn");

chrome.storage.local.get({ enabled: true, autoSniffToClient: true }, (res) => {
  toggleEl.checked = Boolean(res.enabled);
  autoSniffEl.checked = Boolean(res.autoSniffToClient);
});

toggleEl.addEventListener("change", () => {
  chrome.storage.local.set({ enabled: toggleEl.checked });
});

autoSniffEl.addEventListener("change", () => {
  chrome.storage.local.set({ autoSniffToClient: autoSniffEl.checked });
});

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
      sniffSummary.innerHTML = '<span class="pill">未发现直链或流媒体，播放视频后将自动嗅探</span>';
      openSniffBtn.textContent = "打开 Siphon 资源嗅探管理窗";
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
    if (counts.hls > 0) addPill(`HLS 流 ${counts.hls}`, "hls");
    if (counts.video > 0) addPill(`视频 ${counts.video}`, "media");
    if (counts.audio > 0) addPill(`音频 ${counts.audio}`, "media");
    if (counts.file > 0) addPill(`文件 ${counts.file}`);
    if (counts.p2p > 0) addPill(`磁力/eD2k ${counts.p2p}`, "p2p");
    if (counts.image > 0) addPill(`图片 ${counts.image}`);

    openSniffBtn.textContent = `在 Siphon 独立弹窗管理 (${items.length} 项)`;
  });
}

openSniffBtn.addEventListener("click", () => {
  openSniffBtn.textContent = "正在打开 Siphon 嗅探管理窗...";
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
loadPageSummary();
