const SIPHON_ENDPOINT = "http://127.0.0.1:16800";
const MIN_INTERCEPT_SIZE = 256 * 1024; // Only auto-intercept unknown-extension files >= 256 KB
const CAPTURE_EXT_RE = /\.(zip|7z|rar|tar|gz|bz2|xz|tgz|zst|iso|img|vhd|vhdx|dmg|ipsw|exe|msi|msix|appx|deb|rpm|apk|mp4|mkv|webm|mov|avi|flv|ts|m4v|mp3|flac|wav|aac|ogg|m4a|pdf|epub|doc|docx|xls|xlsx|ppt|pptx|bin|pak|safetensors|gguf)(\?|$)/i;
const IMAGE_EXT_RE = /\.(png|jpe?g|webp|gif|bmp|avif|svg|ico)(\?|$)/i;
const HTTP_URL_RE = /https?:\/\/[^\s"'<>()\[\]{}]+/gi;

// Keep track of download IDs already handled to avoid duplicate capture
const handledIds = new Set();

function isHttpUrl(url) {
  return typeof url === "string" && (url.startsWith("http://") || url.startsWith("https://"));
}

async function notifySiphonAlive() {
  try {
    await fetch(`${SIPHON_ENDPOINT}/api/ping`, {
      method: "GET",
      headers: { "X-Siphon-Client": "chrome-extension/0.2.0" }
    });
  } catch (_) {}
}

let badgeTimer = null;
function flashBadge(text, color) {
  try {
    if (badgeTimer) clearTimeout(badgeTimer);
    chrome.action.setBadgeBackgroundColor({ color });
    chrome.action.setBadgeText({ text });
    badgeTimer = setTimeout(() => {
      chrome.action.setBadgeText({ text: "" });
    }, 2600);
  } catch (_) {}
}

function registerContextMenus() {
  chrome.contextMenus.removeAll(() => {
    // 1. Link resource
    chrome.contextMenus.create({
      id: "siphon-download-link",
      title: "使用 Siphon 下载此链接",
      contexts: ["link"]
    });
    chrome.contextMenus.create({
      id: "siphon-download-link-silent",
      title: "使用 Siphon 后台静默下载链接",
      contexts: ["link"]
    });

    // 2. Image resource
    chrome.contextMenus.create({
      id: "siphon-download-image",
      title: "使用 Siphon 下载此图片",
      contexts: ["image"]
    });
    chrome.contextMenus.create({
      id: "siphon-download-image-silent",
      title: "使用 Siphon 后台静默下载图片",
      contexts: ["image"]
    });

    // 3. Video / Audio media resource
    chrome.contextMenus.create({
      id: "siphon-download-media",
      title: "使用 Siphon 下载此音视频",
      contexts: ["video", "audio"]
    });
    chrome.contextMenus.create({
      id: "siphon-download-media-silent",
      title: "使用 Siphon 后台静默下载音视频",
      contexts: ["video", "audio"]
    });

    // 4. Selected text containing URLs
    chrome.contextMenus.create({
      id: "siphon-download-selection",
      title: "使用 Siphon 下载选中文本中的链接",
      contexts: ["selection"]
    });

    // 5. Current page downloadable resources batch scan
    chrome.contextMenus.create({
      id: "siphon-download-page",
      title: "使用 Siphon 抓取当前页可下载资源",
      contexts: ["page", "frame"]
    });
  });
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.set({ enabled: true });
  registerContextMenus();
  notifySiphonAlive();
});

chrome.runtime.onStartup.addListener(() => {
  registerContextMenus();
  notifySiphonAlive();
});

notifySiphonAlive();

function extractHttpUrlsFromText(text) {
  if (!text) return [];
  const matches = text.match(HTTP_URL_RE) || [];
  const seen = new Set();
  const out = [];
  for (let raw of matches) {
    raw = raw.replace(/[.,;:!?)]+$/, "");
    if (isHttpUrl(raw) && !seen.has(raw)) {
      seen.add(raw);
      out.push(raw);
    }
  }
  return out;
}

async function scanTabResources(tabId, mode) {
  if (!tabId) return [];
  try {
    const results = await chrome.scripting.executeScript({
      target: { tabId },
      func: (scanMode) => {
        const extRe = /\.(zip|7z|rar|tar|gz|bz2|xz|tgz|zst|iso|img|vhd|vhdx|dmg|ipsw|exe|msi|msix|appx|deb|rpm|apk|mp4|mkv|webm|mov|avi|flv|ts|m4v|mp3|flac|wav|aac|ogg|m4a|pdf|epub|doc|docx|xls|xlsx|ppt|pptx|bin|pak|safetensors|gguf)(\?|$)/i;
        const found = [];
        const seen = new Set();
        const add = (url, kind, label) => {
          if (!url || (!url.startsWith("http://") && !url.startsWith("https://"))) return;
          if (seen.has(url)) return;
          seen.add(url);
          found.push({ url, kind, label: (label || "").trim().slice(0, 80) });
        };

        // Always collect <video> / <audio> and child <source>
        document.querySelectorAll("video, audio").forEach((el) => {
          const kind = el.tagName.toLowerCase();
          if (el.currentSrc) add(el.currentSrc, kind, el.getAttribute("title") || kind);
          if (el.src) add(el.src, kind, el.getAttribute("title") || kind);
          el.querySelectorAll("source").forEach((src) => {
            if (src.src) add(src.src, kind, src.type || kind);
          });
        });

        if (scanMode === "media") {
          return found;
        }

        // Collect downloadable <a href> links
        document.querySelectorAll("a[href]").forEach((a) => {
          const href = a.href;
          if (a.hasAttribute("download") || extRe.test(href)) {
            add(href, "file", a.getAttribute("download") || a.textContent || "");
          }
        });

        if (scanMode === "all") {
          // Also collect large images (>= 200x200) for popup sniffer
          document.querySelectorAll("img[src]").forEach((img) => {
            if ((img.naturalWidth || 0) >= 200 && (img.naturalHeight || 0) >= 200) {
              add(img.currentSrc || img.src, "image", img.alt || "");
            }
          });
        }

        return found.slice(0, 60);
      },
      args: [mode]
    });
    return (results && results[0] && Array.isArray(results[0].result)) ? results[0].result : [];
  } catch (_) {
    return [];
  }
}

// Handle messages from popup.js (e.g. page resource sniffer & direct sends with cookies)
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (!msg || !msg.type) return;
  if (msg.type === "SCAN_ACTIVE_TAB") {
    chrome.tabs.query({ active: true, currentWindow: true }, async (tabs) => {
      const tab = tabs && tabs[0];
      if (!tab) {
        sendResponse({ ok: false, items: [] });
        return;
      }
      const items = await scanTabResources(tab.id, "all");
      sendResponse({ ok: true, referrer: tab.url || "", items });
    });
    return true;
  }
  if (msg.type === "SUBMIT_TO_SIPHON") {
    sendToSiphon({
      url: msg.url,
      filename: msg.filename || null,
      referrer: msg.referrer || "",
      mime: msg.mime || null,
      fileSize: null,
      silent: Boolean(msg.silent)
    }).then((ok) => {
      flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
      sendResponse({ ok });
    });
    return true;
  }
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const referrer = tab?.url || info.pageUrl || "";
  const id = info.menuItemId;

  if (id === "siphon-download-link" || id === "siphon-download-link-silent") {
    const targetUrl = info.linkUrl;
    if (!isHttpUrl(targetUrl)) return;
    const ok = await sendToSiphon({
      url: targetUrl,
      referrer,
      filename: null,
      mime: null,
      fileSize: null,
      silent: id === "siphon-download-link-silent"
    });
    flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
    return;
  }

  if (id === "siphon-download-image" || id === "siphon-download-image-silent") {
    const targetUrl = info.srcUrl || info.linkUrl;
    if (!isHttpUrl(targetUrl)) return;
    const ok = await sendToSiphon({
      url: targetUrl,
      referrer,
      filename: null,
      mime: "image/*",
      fileSize: null,
      silent: id === "siphon-download-image-silent"
    });
    flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
    return;
  }

  if (id === "siphon-download-media" || id === "siphon-download-media-silent") {
    const silent = id === "siphon-download-media-silent";
    let targetUrl = info.srcUrl;
    if (!isHttpUrl(targetUrl) && tab?.id) {
      const media = await scanTabResources(tab.id, "media");
      if (media.length > 0) {
        targetUrl = media[0].url;
      }
    }
    if (!isHttpUrl(targetUrl)) {
      flashBadge("0", "#E5A93B");
      return;
    }
    const ok = await sendToSiphon({
      url: targetUrl,
      referrer,
      filename: null,
      mime: info.mediaType === "audio" ? "audio/*" : "video/*",
      fileSize: null,
      silent
    });
    flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
    return;
  }

  if (id === "siphon-download-selection") {
    const urls = extractHttpUrlsFromText(info.selectionText || "");
    if (urls.length === 0) {
      flashBadge("0", "#E5A93B");
      return;
    }
    if (urls.length === 1) {
      const ok = await sendToSiphon({
        url: urls[0],
        referrer,
        filename: null,
        mime: null,
        fileSize: null,
        silent: false
      });
      flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
    } else {
      let sent = 0;
      for (const u of urls) {
        const ok = await sendToSiphon({
          url: u,
          referrer,
          filename: null,
          mime: null,
          fileSize: null,
          silent: true
        });
        if (ok) sent += 1;
      }
      flashBadge(sent > 0 ? String(sent) : "!", sent > 0 ? "#22C55E" : "#E55353");
    }
    return;
  }

  if (id === "siphon-download-page" && tab?.id) {
    const resources = await scanTabResources(tab.id, "downloadable");
    if (resources.length === 0) {
      flashBadge("0", "#E5A93B");
      return;
    }
    if (resources.length === 1) {
      const ok = await sendToSiphon({
        url: resources[0].url,
        referrer,
        filename: null,
        mime: null,
        fileSize: null,
        silent: false
      });
      flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
    } else {
      let sent = 0;
      for (const res of resources) {
        const ok = await sendToSiphon({
          url: res.url,
          referrer,
          filename: null,
          mime: null,
          fileSize: null,
          silent: true
        });
        if (ok) sent += 1;
      }
      flashBadge(sent > 0 ? String(sent) : "!", sent > 0 ? "#22C55E" : "#E55353");
    }
  }
});

const IGNORE_EXT_RE = /\.(html?|mhtml|xhtml|css|js|json|xml)(\?|$)/i;

function shouldCaptureDownload(item, fromDeterminingFilename = false) {
  if (!item || item.byExtensionId) return false;
  const url = item.finalUrl || item.url || "";
  if (!isHttpUrl(url)) {
    return false;
  }
  if (url.startsWith("http://127.0.0.1") || url.startsWith("http://localhost")) {
    return false;
  }
  const fname = basename(item.filename) || "";
  if (fname && IGNORE_EXT_RE.test(fname)) {
    return false;
  }
  if (CAPTURE_EXT_RE.test(url) || (fname && CAPTURE_EXT_RE.test(fname))) {
    return true;
  }
  const mime = (item.mime || "").toLowerCase();
  if (
    mime.startsWith("application/octet-stream") ||
    mime.startsWith("application/x-") ||
    mime.startsWith("application/vnd.") ||
    mime.startsWith("application/zip") ||
    mime.startsWith("application/gzip") ||
    mime.startsWith("application/pdf") ||
    mime.startsWith("binary/") ||
    mime.startsWith("video/") ||
    mime.startsWith("audio/")
  ) {
    return true;
  }
  if (typeof item.fileSize === "number" && item.fileSize >= 64 * 1024) {
    return true;
  }
  // When Chrome already decided to save a file with a real filename (and not an HTML webpage),
  // capture it even if Content-Length is unknown (-1)
  if (fromDeterminingFilename && fname) {
    return true;
  }
  return false;
}

async function getCookieHeader(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    if (!cookies || cookies.length === 0) return "";
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch (_) {
    return "";
  }
}

function basename(pathStr) {
  if (!pathStr) return null;
  const normalized = pathStr.replace(/\\/g, "/");
  const last = normalized.split("/").pop();
  return last || null;
}

async function sendToSiphon({ url, filename, referrer, mime, fileSize, silent = false }) {
  try {
    const cookie = await getCookieHeader(url);
    const resp = await fetch(`${SIPHON_ENDPOINT}/api/capture`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Siphon-Client": "chrome-extension/0.2.0"
      },
      body: JSON.stringify({
        url,
        filename: basename(filename),
        referrer: referrer || null,
        cookie: cookie || null,
        user_agent: navigator.userAgent,
        mime: mime || null,
        file_size: typeof fileSize === "number" && fileSize > 0 ? fileSize : null,
        silent: Boolean(silent)
      })
    });
    if (!resp.ok) return false;
    const data = await resp.json();
    return Boolean(data && data.ok && data.captured);
  } catch (_) {
    return false;
  }
}

// Listen in onDeterminingFilename so we get the server's resolved Content-Disposition filename & size
chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  suggest();
  handleDownloadItem(item, true);
});

chrome.downloads.onCreated.addListener((item) => {
  // Fallback for browsers that do not fire onDeterminingFilename: re-query latest item metadata
  setTimeout(async () => {
    if (!item || handledIds.has(item.id)) return;
    try {
      const results = await chrome.downloads.search({ id: item.id });
      if (results && results[0]) {
        await handleDownloadItem(results[0], false);
      }
    } catch (_) {
      await handleDownloadItem(item, false);
    }
  }, 350);
});

async function handleDownloadItem(item, fromDeterminingFilename = false) {
  if (!item || handledIds.has(item.id)) return;
  if (item.state && item.state !== "in_progress") return;

  const st = await chrome.storage.local.get({ enabled: true });
  if (!st.enabled) return;

  if (!shouldCaptureDownload(item, fromDeterminingFilename)) return;
  handledIds.add(item.id);

  // Pause browser native transfer while handing off to Siphon so small files don't race to completion
  try {
    await chrome.downloads.pause(item.id);
  } catch (_) {}

  const url = item.finalUrl || item.url;
  const captured = await sendToSiphon({
    url,
    filename: item.filename,
    referrer: item.referrer,
    mime: item.mime,
    fileSize: item.fileSize,
    silent: false
  });

  if (captured) {
    flashBadge("OK", "#22C55E");
    try {
      await chrome.downloads.cancel(item.id);
      await chrome.downloads.erase({ id: item.id });
    } catch (_) {}
  } else {
    handledIds.delete(item.id);
    try {
      await chrome.downloads.resume(item.id);
    } catch (_) {}
  }
}
