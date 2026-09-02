// Service worker for Impressive Download Manager Extension - Phase 5 (HTTPS Hero Engine)
const PORT = 9600;

let extensionEnabled = true;
let desktopAppInterceptionDisabled = false;
const handledDownloadIds = new Set();
const webRequestCapturedUrls = new Map(); // filename -> direct HTTPS URL

// Check desktop app interception setting every 4 seconds
async function checkDesktopAppStatus() {
  try {
    const res = await fetch(`http://127.0.0.1:${PORT}/status`, { cache: "no-store" });
    if (res.ok) {
      const data = await res.json();
      desktopAppInterceptionDisabled = data.enabled === false;
    } else if (res.status === 403) {
      desktopAppInterceptionDisabled = true;
    }
  } catch (e) {
    // If backend is offline, leave extensionEnabled behavior intact
  }
}
setInterval(checkDesktopAppStatus, 4000);
checkDesktopAppStatus();

// Initialize state
chrome.storage.local.get("extensionEnabled", (data) => {
  if (data.extensionEnabled !== undefined) {
    extensionEnabled = data.extensionEnabled;
  } else {
    chrome.storage.local.set({ extensionEnabled: true });
  }
});

// Watch changes
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && changes.extensionEnabled) {
    extensionEnabled = changes.extensionEnabled.newValue;
  }
});

// Reset state on startup/install
chrome.runtime.onStartup.addListener(() => {
  chrome.storage.local.set({ extensionEnabled: true });
});

chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.set({ extensionEnabled: true });
});

// Helper to extract cookie headers for a target URL and active tab
async function getCookiesForUrl(url, tabUrl) {
  try {
    const cookieMap = new Map();

    // 1. Get cookies for target download URL
    if (url && (url.startsWith("http://") || url.startsWith("https://"))) {
      try {
        const cookies1 = await chrome.cookies.getAll({ url });
        if (cookies1) {
          for (const c of cookies1) {
            cookieMap.set(c.name, c.value);
          }
        }
      } catch (e) {}

      // Also get cookies for root domain if subdomain
      try {
        const u = new URL(url);
        const parts = u.hostname.split(".");
        if (parts.length > 2) {
          const domain = parts.slice(-2).join(".");
          const domainCookies = await chrome.cookies.getAll({ domain });
          if (domainCookies) {
            for (const c of domainCookies) {
              if (!cookieMap.has(c.name)) cookieMap.set(c.name, c.value);
            }
          }
        }
      } catch (e) {}
    }

    // 2. Also get cookies from active tab URL if different
    if (tabUrl && (tabUrl.startsWith("http://") || tabUrl.startsWith("https://"))) {
      try {
        const cookies2 = await chrome.cookies.getAll({ url: tabUrl });
        if (cookies2) {
          for (const c of cookies2) {
            if (!cookieMap.has(c.name)) {
              cookieMap.set(c.name, c.value);
            }
          }
        }
      } catch (e) {}
    }

    const merged = Array.from(cookieMap.entries())
      .map(([name, value]) => `${name}=${value}`)
      .join("; ");
    return merged;
  } catch (err) {
    console.warn("Failed to get cookies:", err);
    return "";
  }
}

// 1. Non-blocking webRequest listener for HTTPS direct mirror signals
if (chrome.webRequest && chrome.webRequest.onBeforeRequest) {
  try {
    chrome.webRequest.onBeforeRequest.addListener(
      (details) => {
        if (!details.url) return;
        const u = details.url;
        const filename = u.substring(u.lastIndexOf("/") + 1).split("?")[0];
        if (filename && filename.length > 3) {
          webRequestCapturedUrls.set(filename, u);
          console.log("[HTTPS Direct WebRequest Signal]", filename, "->", u);
        }
      },
      {
        urls: [
          "*://downloads.sourceforge.net/*",
          "*://*.dl.sourceforge.net/*",
          "*://objects.githubusercontent.com/*"
        ]
      }
    );
  } catch (e) {
    console.warn("webRequest listener failed to attach:", e);
  }
}

// 2. HTTPS Download Detection Pipeline (Instant Interception via onDeterminingFilename)
async function interceptDownloadItem(item) {
  if (!item || !extensionEnabled || desktopAppInterceptionDisabled || handledDownloadIds.has(item.id)) {
    return false;
  }
  if (item.state === "interrupted" || item.state === "complete") {
    return false;
  }

  const url = item.url || "";
  const finalUrl = item.finalUrl || url;

  // Reject non-HTTP(S) protocols (blob:, data:, file:, chrome:) so the browser handles in-memory objects natively
  if (!url.startsWith("http://") && !url.startsWith("https://") && !finalUrl.startsWith("http://") && !finalUrl.startsWith("https://")) {
    console.log("[Extension] In-memory or non-HTTP(S) protocol (e.g. blob:, data:). Letting browser save natively:", url);
    return false;
  }

  const mime = item.mime || "";
  const filenameStr = item.filename ? item.filename.replace(/^.*[\\\/]/, "") : "";
  const fileSize = (item.fileSize && item.fileSize > 0) ? item.fileSize : (item.totalBytes && item.totalBytes > 0 ? item.totalBytes : 0);

  // Check direct webRequest mirror signal
  const directUrlSignal = filenameStr ? webRequestCapturedUrls.get(filenameStr) : null;
  let targetRealUrl = directUrlSignal || finalUrl;
  if ((url.includes("sourceforge.net") || targetRealUrl.includes("sourceforge.net")) && url.includes("/project/")) {
    targetRealUrl = url;
  }

  // Reject HTML web pages and redirector landing URLs strictly
  if (mime === "text/html" || mime === "text/plain" || mime.includes("html")) {
    return false;
  }

  // Use URL parsing to strictly reject SourceForge landing pages even with query strings
  try {
    const pUrl = new URL(targetRealUrl);
    if (pUrl.hostname.includes("sourceforge.net") && pUrl.pathname.endsWith("/download")) {
      return false;
    }
  } catch (e) {}

  const cleanFilename = filenameStr || extractFilename(targetRealUrl);

  const isBinaryExt =
    /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage|7z|rar|bz2|xz|pdf|docx?|xlsx?|pptx?|mp4|mkv|webm|mp3|wav|flac|bin|pkg|csv|epub|mobi)$/i.test(cleanFilename) ||
    /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage|7z|rar|bz2|xz|pdf|docx?|xlsx?|pptx?|mp4|mkv|webm|mp3|wav|flac|bin|pkg|csv|epub|mobi)$/i.test(url) ||
    /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage|7z|rar|bz2|xz|pdf|docx?|xlsx?|pptx?|mp4|mkv|webm|mp3|wav|flac|bin|pkg|csv|epub|mobi)$/i.test(targetRealUrl);
  const isNonTextMime = mime && !mime.startsWith("text/");
  const hasSubstantialSize = fileSize >= 1024 * 1024;

  const isRealFile = (hasSubstantialSize || isNonTextMime || isBinaryExt);

  if (isRealFile) {
    handledDownloadIds.add(item.id);

    // Cancel & erase browser download IMMEDIATELY before Chrome UI pops up
    try {
      chrome.downloads.cancel(item.id, () => {
        try {
          chrome.downloads.erase({ id: item.id }, () => {});
        } catch (e) {}
      });
    } catch (e) {}

    let tabUrl = "";
    let referrer = item.referrer || "";
    try {
      const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
      if (tabs && tabs[0]) {
        tabUrl = tabs[0].url || "";
        if (!referrer) {
          referrer = tabUrl;
        }
      }
    } catch (e) {}

    const cookies = await getCookiesForUrl(targetRealUrl, tabUrl);

    const payload = {
      url: targetRealUrl,
      filename: cleanFilename,
      size: fileSize,
      mime: mime,
      referrer: referrer,
      cookie: cookies,
      userAgent: navigator.userAgent
    };

    console.log("HTTPS HERO: INSTANT REAL FILE PAYLOAD SENT TO TAURI BACKEND:", payload);
    await sendToDesktopApp(payload);
    return true;
  }
  return false;
}

if (chrome.downloads && chrome.downloads.onDeterminingFilename) {
  chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
    (async () => {
      const intercepted = await interceptDownloadItem(item);
      if (!intercepted && typeof suggest === "function") {
        suggest();
      }
    })();
    return true;
  });
}

const recentInterceptions = new Map();

// Helper to send payloads to Tauri backend (port 9600)
async function sendToDesktopApp(payload) {
  const url = payload.url;
  const now = Date.now();
  const lastIntercept = recentInterceptions.get(url);
  if (lastIntercept && now - lastIntercept < 2500) {
    console.log("De-duplicated download payload for URL:", url);
    return false;
  }

  for (const [key, time] of recentInterceptions.entries()) {
    if (now - time > 15000) {
      recentInterceptions.delete(key);
    }
  }

  for (let attempt = 1; attempt <= 2; attempt++) {
    try {
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), 1500);

      const response = await fetch(`http://127.0.0.1:${PORT}/download`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json"
        },
        mode: "cors",
        signal: controller.signal,
        body: JSON.stringify(payload)
      });
      clearTimeout(timeoutId);

      if (response.ok) {
        recentInterceptions.set(url, now);
        const result = await response.json();
        console.log("Tauri backend response:", result);
        return true;
      } else if (response.status === 403) {
        console.log("Download interception is toggled OFF in desktop app settings.");
        return false;
      }
    } catch (err) {
      if (attempt === 1) {
        console.warn("Tauri backend offline on port 9600. Trying wakeup...");
        try {
          chrome.tabs.create({ url: "idm://wakeup", active: false }, (tab) => {
            if (tab && tab.id) {
              setTimeout(() => {
                chrome.tabs.remove(tab.id, () => { });
              }, 400);
            }
          });
        } catch (e) { }
        await new Promise((r) => setTimeout(r, 800));
      }
    }
  }
  return false;
}

function extractFilename(url) {
  try {
    const parsed = new URL(url);
    // Check if filename is present in query parameters (e.g. ?filename=..., ?file=..., ?name=...)
    for (const [key, val] of parsed.searchParams.entries()) {
      const k = key.toLowerCase();
      if ((k.includes("file") || k.includes("name") || k.includes("title")) && val.includes(".")) {
        const seg = val.split("/").pop()?.split("\\").pop();
        if (seg && seg.includes(".")) {
          try {
            return decodeURIComponent(seg);
          } catch {
            return seg;
          }
        }
      }
    }

    const pathname = parsed.pathname;
    const last = pathname.substring(pathname.lastIndexOf("/") + 1).split("?")[0];
    if (last && last !== "download") {
      try {
        return decodeURIComponent(last);
      } catch {
        return last;
      }
    }
    return "captured_download";
  } catch {
    return "captured_download";
  }
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === "SEND_TO_DESKTOP") {
    if (!extensionEnabled) {
      sendResponse({ success: false });
      return true;
    }
    (async () => {
      const cookies = await getCookiesForUrl(message.url);
      const payload = {
        url: message.url,
        filename: message.filename || extractFilename(message.url),
        size: 0,
        mime: "",
        referrer: "",
        cookie: cookies,
        userAgent: navigator.userAgent
      };
      const success = await sendToDesktopApp(payload);
      sendResponse({ success });
    })();
    return true;
  }
});
