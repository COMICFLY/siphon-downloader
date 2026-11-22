const ENDPOINT = "http://127.0.0.1:16800";
const dot = document.getElementById("dot");
const statusEl = document.getElementById("status");
const toggleEl = document.getElementById("toggle");
const urlInput = document.getElementById("urlInput");
const sendBtn = document.getElementById("sendBtn");
const resList = document.getElementById("resList");
const resCount = document.getElementById("resCount");

chrome.storage.local.get({ enabled: true }, (res) => {
  toggleEl.checked = Boolean(res.enabled);
});

toggleEl.addEventListener("change", () => {
  chrome.storage.local.set({ enabled: toggleEl.checked });
});

async function checkStatus() {
  try {
    const resp = await fetch(`${ENDPOINT}/api/ping`);
    const data = await resp.json();
    if (data && data.ok) {
      dot.classList.add("ok");
      statusEl.textContent = data.enabled ? "已连接" : "已在设置关闭";
      return;
    }
  } catch (_) {}
  dot.classList.remove("ok");
  statusEl.textContent = "未启动";
}

function shortName(url, fallback) {
  if (fallback && fallback.trim()) return fallback.trim();
  try {
    const u = new URL(url);
    const seg = decodeURIComponent(u.pathname.split("/").filter(Boolean).pop() || u.hostname);
    return seg || url;
  } catch (_) {
    return url;
  }
}

function loadPageResources() {
  chrome.runtime.sendMessage({ type: "SCAN_ACTIVE_TAB" }, (resp) => {
    if (chrome.runtime.lastError || !resp || !resp.ok || !resp.items.length) {
      resCount.textContent = "0 项";
      resList.innerHTML = '<div class="empty">当前页未发现可嗅探的直链或媒体资源</div>';
      return;
    }
    const referrer = resp.referrer || "";
    resCount.textContent = `${resp.items.length} 项`;
    resList.innerHTML = "";
    for (const item of resp.items) {
      const row = document.createElement("div");
      row.className = "res-item";

      const meta = document.createElement("div");
      meta.className = "res-meta";
      const title = document.createElement("div");
      title.className = "res-title";
      title.title = item.url;

      const tag = document.createElement("span");
      tag.className = `res-tag ${item.kind || ""}`;
      tag.textContent = item.kind;
      title.appendChild(tag);
      title.appendChild(document.createTextNode(shortName(item.url, item.label)));
      meta.appendChild(title);

      const btn = document.createElement("button");
      btn.className = "res-btn";
      btn.textContent = "下载";
      btn.addEventListener("click", () => {
        btn.textContent = "已发送";
        chrome.runtime.sendMessage({
          type: "SUBMIT_TO_SIPHON",
          url: item.url,
          referrer,
          silent: false
        });
      });

      row.appendChild(meta);
      row.appendChild(btn);
      resList.appendChild(row);
    }
  });
}

sendBtn.addEventListener("click", async () => {
  const url = urlInput.value.trim();
  const low = url.toLowerCase();
  if (!low.startsWith("http://") && !low.startsWith("https://") && !low.startsWith("magnet:?")) {
    return;
  }
  chrome.runtime.sendMessage(
    { type: "SUBMIT_TO_SIPHON", url, referrer: "", silent: false },
    () => {
      urlInput.value = "";
      window.close();
    }
  );
});

checkStatus();
loadPageResources();
