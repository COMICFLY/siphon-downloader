const SIPHON_ENDPOINT = "http://127.0.0.1:16800";
const NATIVE_HOST_NAME = "com.siphon.downloader";
const CLIENT_VERSION = "chrome-extension/0.5.0";

const CAPTURE_EXT_RE = /\.(zip|7z|rar|tar|gz|bz2|xz|tgz|zst|iso|img|vhd|vhdx|dmg|ipsw|exe|msi|msix|appx|deb|rpm|apk|mp4|mkv|webm|mov|avi|flv|ts|m4v|m3u8|mp3|flac|wav|aac|ogg|m4a|pdf|epub|doc|docx|xls|xlsx|ppt|pptx|torrent|bin|pak|safetensors|gguf)(\?|$)/i;
const M3U8_URL_RE = /\.m3u8(\?|$)/i;
const VIDEO_URL_RE = /\.(mp4|mkv|webm|mov|avi|flv|m4v)(\?|$)/i;
const AUDIO_URL_RE = /\.(mp3|flac|wav|aac|ogg|m4a)(\?|$)/i;
const DOWNLOADABLE_URL_RE = /(?:https?:\/\/[^\s"'<>()\[\]{}]+|magnet:\?[^\s"'<>()\[\]{}]+|ed2k:\/\/\|file\|[^\s"'<>()\[\]{}]+)/gi;

// Keep track of download IDs already handled to avoid duplicate capture
const handledIds = new Set();

// Live-sniffed network streams per browser tab (HLS / video / audio)
const tabSniffedStreams = new Map();
// Track tabs that already auto-popped the client sniff window for the current page URL
const tabAutoPoppedUrl = new Map();
const tabAutoTimers = new Map();

function isHttpUrl(url) {
  return typeof url === "string" && (url.startsWith("http://") || url.startsWith("https://"));
}

function isDownloadableUrl(url) {
  if (typeof url !== "string") return false;
  const low = url.toLowerCase();
  return (
    low.startsWith("http://") ||
    low.startsWith("https://") ||
    low.startsWith("magnet:?") ||
    low.startsWith("ed2k://|file|")
  );
}

function bridgeHeaders(extra = {}) {
  const h = {
    "X-Siphon-Client": CLIENT_VERSION,
    ...extra
  };
  try {
    if (chrome.runtime && chrome.runtime.id) {
      h["X-Siphon-Extension-Id"] = chrome.runtime.id;
    }
  } catch (_) {}
  return h;
}

function sendNativeToSiphon(ipcCommand) {
  return new Promise((resolve) => {
    try {
      chrome.runtime.sendNativeMessage(NATIVE_HOST_NAME, ipcCommand, (reply) => {
        if (chrome.runtime.lastError) {
          resolve({ ok: false, launched: false, error: chrome.runtime.lastError.message || "native_error" });
          return;
        }
        resolve(reply || { ok: false, launched: false });
      });
    } catch (e) {
      resolve({ ok: false, launched: false, error: String(e) });
    }
  });
}

function recordNetworkStream(tabId, item) {
  if (typeof tabId !== "number" || tabId < 0 || !isHttpUrl(item.url)) return false;
  if (item.url.startsWith("http://127.0.0.1") || item.url.startsWith("http://localhost")) return false;

  let list = tabSniffedStreams.get(tabId);
  if (!list) {
    list = [];
    tabSniffedStreams.set(tabId, list);
  }
  const existing = list.find((x) => x.url === item.url);
  if (existing) {
    if (!existing.file_size && item.file_size) existing.file_size = item.file_size;
    if (!existing.mime && item.mime) existing.mime = item.mime;
    return false;
  }

  list.unshift({
    url: item.url,
    filename: item.filename || null,
    kind: item.kind || "video",
    label: item.label || null,
    mime: item.mime || null,
    file_size: typeof item.file_size === "number" && item.file_size > 0 ? item.file_size : null
  });
  if (list.length > 48) {
    list.length = 48;
  }
  return true;
}

function scheduleAutoSniffPush(tabId) {
  if (tabAutoTimers.has(tabId)) {
    clearTimeout(tabAutoTimers.get(tabId));
  }
  const timer = setTimeout(async () => {
    tabAutoTimers.delete(tabId);
    const st = await chrome.storage.local.get({ autoSniffToClient: true });
    if (!st.autoSniffToClient) return;

    let tab = null;
    try {
      tab = await chrome.tabs.get(tabId);
    } catch (_) {
      return;
    }
    if (!tab || !tab.active || !tab.url || !isHttpUrl(tab.url)) return;

    const streams = (tabSniffedStreams.get(tabId) || []).slice();
    if (streams.length === 0) return;

    // Bring client Sniff Manager popup to front once per page URL; merge silently for extra streams
    const alreadyPopped = tabAutoPoppedUrl.get(tabId) === tab.url;
    tabAutoPoppedUrl.set(tabId, tab.url);

    const ok = await sendSniffToSiphon(tab, streams, !alreadyPopped);
    if (ok) {
      flashBadge(String(streams.length), "#22C55E");
    }
  }, 550);
  tabAutoTimers.set(tabId, timer);
}

// Sniff .m3u8 playlist URLs immediately on request
chrome.webRequest.onBeforeRequest.addListener(
  (details) => {
    if (details.tabId >= 0 && M3U8_URL_RE.test(details.url)) {
      const added = recordNetworkStream(details.tabId, {
        url: details.url,
        kind: "hls",
        label: "HLS Stream (.m3u8)",
        mime: "application/vnd.apple.mpegurl"
      });
      if (added) scheduleAutoSniffPush(details.tabId);
    }
  },
  { urls: ["<all_urls>"] }
);

// Sniff HLS, video, and audio streams via Content-Type & Content-Length headers
chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0 || !details.responseHeaders) return;
    let contentType = "";
    let contentLength = null;
    for (const h of details.responseHeaders) {
      if (!h.name || !h.value) continue;
      const name = h.name.toLowerCase();
      if (name === "content-type") {
        contentType = h.value.toLowerCase().split(";")[0].trim();
      } else if (name === "content-length") {
        const parsed = parseInt(h.value, 10);
        if (!Number.isNaN(parsed) && parsed > 0) contentLength = parsed;
      }
    }

    let kind = null;
    let label = null;
    if (
      contentType.includes("application/vnd.apple.mpegurl") ||
      contentType.includes("application/x-mpegurl") ||
      contentType.includes("audio/mpegurl") ||
      M3U8_URL_RE.test(details.url)
    ) {
      kind = "hls";
      label = "HLS Stream (.m3u8)";
    } else if (
      (contentType.startsWith("video/") && !contentType.includes("mp2t")) ||
      VIDEO_URL_RE.test(details.url)
    ) {
      // Ignore tiny video beacons (< 256 KB) when Content-Length is known
      if (contentLength === null || contentLength >= 256 * 1024) {
        kind = "video";
        label = "视频流媒体";
      }
    } else if (contentType.startsWith("audio/") || AUDIO_URL_RE.test(details.url)) {
      if (contentLength === null || contentLength >= 64 * 1024) {
        kind = "audio";
        label = "音频资源";
      }
    }

    if (kind) {
      const added = recordNetworkStream(details.tabId, {
        url: details.url,
        kind,
        label,
        mime: contentType || null,
        file_size: contentLength
      });
      if (added) scheduleAutoSniffPush(details.tabId);
    }
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"]
);

chrome.tabs.onRemoved.addListener((tabId) => {
  tabSniffedStreams.delete(tabId);
  tabAutoPoppedUrl.delete(tabId);
  if (tabAutoTimers.has(tabId)) {
    clearTimeout(tabAutoTimers.get(tabId));
    tabAutoTimers.delete(tabId);
  }
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === "loading" && changeInfo.url) {
    tabSniffedStreams.delete(tabId);
    tabAutoPoppedUrl.delete(tabId);
  }
});

async function notifySiphonAlive() {
  try {
    await fetch(`${SIPHON_ENDPOINT}/api/ping`, {
      method: "GET",
      headers: bridgeHeaders()
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
    // 1. Link resource (HTTP / Magnet / Torrent / HLS / eD2k)
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

    // 4. Selected text containing URLs, magnet, or ed2k links
    chrome.contextMenus.create({
      id: "siphon-download-selection",
      title: "使用 Siphon 下载选中文本中的链接",
      contexts: ["selection"]
    });

    // 5. Hand off all sniffed page resources to Siphon's standalone Sniff Manager window
    chrome.contextMenus.create({
      id: "siphon-sniff-page",
      title: "使用 Siphon 嗅探当前页资源（独立弹窗管理）",
      contexts: ["page", "frame", "video", "audio"]
    });
  });
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.set({ enabled: true, autoSniffToClient: true });
  registerContextMenus();
  notifySiphonAlive();
});

chrome.runtime.onStartup.addListener(() => {
  registerContextMenus();
  notifySiphonAlive();
});

notifySiphonAlive();

function extractUrlsFromText(text) {
  if (!text) return [];
  const matches = text.match(DOWNLOADABLE_URL_RE) || [];
  const seen = new Set();
  const out = [];
  for (let raw of matches) {
    raw = raw.replace(/[.,;:!?)]+$/, "");
    if (isDownloadableUrl(raw) && !seen.has(raw)) {
      seen.add(raw);
      out.push(raw);
    }
  }
  return out;
}

async function scanTabResources(tabId, mode) {
  if (!tabId) return [];
  const netSniffed = (tabSniffedStreams.get(tabId) || []).slice();
  let domItems = [];
  try {
    const results = await chrome.scripting.executeScript({
      target: { tabId },
      func: (scanMode) => {
        const extRe = /\.(zip|7z|rar|tar|gz|bz2|xz|tgz|zst|iso|img|vhd|vhdx|dmg|ipsw|exe|msi|msix|appx|deb|rpm|apk|mp4|mkv|webm|mov|avi|flv|ts|m4v|m3u8|mp3|flac|wav|aac|ogg|m4a|pdf|epub|doc|docx|xls|xlsx|ppt|pptx|torrent|bin|pak|safetensors|gguf)(\?|$)/i;
        const found = [];
        const seen = new Set();
        const add = (url, kind, label) => {
          if (!url) return;
          const low = url.toLowerCase();
          if (
            !low.startsWith("http://") &&
            !low.startsWith("https://") &&
            !low.startsWith("magnet:?") &&
            !low.startsWith("ed2k://|file|")
          ) {
            return;
          }
          if (seen.has(url)) return;
          seen.add(url);
          found.push({
            url,
            filename: null,
            kind,
            label: (label || "").trim().slice(0, 80) || null,
            mime: null,
            file_size: null
          });
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

        // Collect downloadable <a href> links, magnets, ed2k, and torrents
        document.querySelectorAll("a[href]").forEach((a) => {
          const href = a.href;
          if (!href) return;
          const low = href.toLowerCase();
          if (low.startsWith("magnet:?")) {
            add(href, "magnet", a.textContent || "Magnet Link");
          } else if (low.startsWith("ed2k://|file|")) {
            add(href, "ed2k", a.textContent || "eD2k Link");
          } else if (a.hasAttribute("download") || extRe.test(href)) {
            const kind = /\.m3u8(\?|$)/i.test(href)
              ? "hls"
              : /\.torrent(\?|$)/i.test(href)
                ? "torrent"
                : "file";
            add(href, kind, a.getAttribute("download") || a.textContent || "");
          }
        });

        if (scanMode === "all") {
          // Also collect large images (>= 200x200)
          document.querySelectorAll("img[src]").forEach((img) => {
            if ((img.naturalWidth || 0) >= 200 && (img.naturalHeight || 0) >= 200) {
              add(img.currentSrc || img.src, "image", img.alt || "");
            }
          });
        }

        return found.slice(0, 80);
      },
      args: [mode]
    });
    domItems = (results && results[0] && Array.isArray(results[0].result)) ? results[0].result : [];
  } catch (_) {
    domItems = [];
  }

  // Merge live network-sniffed HLS/video/audio streams at the top, deduplicated by URL
  const merged = [];
  const seen = new Set();
  for (const item of [...netSniffed, ...domItems]) {
    if (!seen.has(item.url)) {
      seen.add(item.url);
      merged.push(item);
    }
  }
  return merged.slice(0, 120);
}

// Handle messages from popup.js
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
      sendResponse({
        ok: true,
        pageTitle: tab.title || "",
        referrer: tab.url || "",
        items
      });
    });
    return true;
  }

  if (msg.type === "OPEN_SNIFF_WINDOW") {
    chrome.tabs.query({ active: true, currentWindow: true }, async (tabs) => {
      const tab = tabs && tabs[0];
      if (!tab) {
        sendResponse({ ok: false, count: 0 });
        return;
      }
      const items = await scanTabResources(tab.id, "all");
      const ok = await sendSniffToSiphon(tab, items, true);
      flashBadge(ok ? String(items.length || "OK") : "!", ok ? "#22C55E" : "#E55353");
      sendResponse({ ok, count: items.length });
    });
    return true;
  }

  if (msg.type === "LAUNCH_SIPHON") {
    sendNativeToSiphon({ type: "launch" }).then((reply) => {
      const ok = Boolean(reply && reply.ok);
      flashBadge(ok ? "GO" : "!", ok ? "#22C55E" : "#E55353");
      sendResponse({ ok, reply });
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
    if (!isDownloadableUrl(targetUrl)) return;
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
    const urls = extractUrlsFromText(info.selectionText || "");
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
      // Hand off multi-URL selection to Siphon's standalone Sniff Manager window!
      const items = urls.map((u) => ({
        url: u,
        filename: null,
        kind: u.toLowerCase().startsWith("magnet:?")
          ? "magnet"
          : u.toLowerCase().startsWith("ed2k://")
            ? "ed2k"
            : /\.m3u8(\?|$)/i.test(u)
              ? "hls"
              : "file",
        label: null,
        mime: null,
        file_size: null
      }));
      const ok = await sendSniffToSiphon(tab || { title: "选中链接", url: referrer }, items, true);
      flashBadge(ok ? String(items.length) : "!", ok ? "#22C55E" : "#E55353");
    }
    return;
  }

  if (id === "siphon-sniff-page" && tab?.id) {
    const resources = await scanTabResources(tab.id, "all");
    const ok = await sendSniffToSiphon(tab, resources, true);
    flashBadge(ok ? String(resources.length || "OK") : "!", ok ? "#22C55E" : "#E55353");
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
  if (fromDeterminingFilename && fname) {
    return true;
  }
  return false;
}

async function getCookieHeader(url) {
  if (!isHttpUrl(url)) return "";
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
  const cookie = await getCookieHeader(url);
  const payload = {
    url,
    filename: basename(filename),
    referrer: referrer || null,
    cookie: cookie || null,
    user_agent: navigator.userAgent,
    mime: mime || null,
    file_size: typeof fileSize === "number" && fileSize > 0 ? fileSize : null,
    silent: Boolean(silent)
  };

  // 1. Fast path: local HTTP bridge when Siphon is already running
  try {
    const resp = await fetch(`${SIPHON_ENDPOINT}/api/capture`, {
      method: "POST",
      headers: bridgeHeaders({ "Content-Type": "application/json" }),
      body: JSON.stringify(payload)
    });
    if (resp.ok) {
      const data = await resp.json();
      return Boolean(data && data.ok && data.captured);
    }
  } catch (_) {
    // Client not running -> auto-launch via Native Messaging Host and deliver payload!
    const nativeReply = await sendNativeToSiphon({
      type: "browser_capture",
      ...payload
    });
    return Boolean(nativeReply && nativeReply.ok);
  }
  return false;
}

async function sendSniffToSiphon(tab, items, bringToFront = true) {
  const pageUrl = (tab && tab.url) || "";
  const cookie = await getCookieHeader(pageUrl);
  const payload = {
    page_title: (tab && tab.title) || null,
    page_url: pageUrl || null,
    referrer: pageUrl || null,
    cookie: cookie || null,
    user_agent: navigator.userAgent,
    items: Array.isArray(items) ? items : [],
    bring_to_front: Boolean(bringToFront)
  };

  // 1. Fast path: local HTTP bridge when Siphon is already running
  try {
    const resp = await fetch(`${SIPHON_ENDPOINT}/api/sniff`, {
      method: "POST",
      headers: bridgeHeaders({ "Content-Type": "application/json" }),
      body: JSON.stringify(payload)
    });
    if (resp.ok) {
      const data = await resp.json();
      return Boolean(data && data.ok);
    }
  } catch (_) {
    // Client not running -> auto-launch Siphon via Native Messaging Host and open Sniff Manager popup!
    const nativeReply = await sendNativeToSiphon({
      type: "browser_sniff",
      ...payload
    });
    return Boolean(nativeReply && nativeReply.ok);
  }
  return false;
}

chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  suggest();
  handleDownloadItem(item, true);
});

chrome.downloads.onCreated.addListener((item) => {
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
