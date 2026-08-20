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

// Helper to extract cookie headers for a target URL
async function getCookiesForUrl(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
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

// Helper to probe chunked / unknown size streams (e.g. Google Docs/Drive exports)
async function checkUnknownSizeIsSmall(url, cookies) {
  const ONE_MB = 1024 * 1024;
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), 2500);

  try {
    const headers = {};
    if (cookies) {
      headers["Cookie"] = cookies;
    }
    const res = await fetch(url, {
      method: "GET",
      headers,
      signal: controller.signal,
      credentials: "include"
    });

    clearTimeout(timeoutId);

    if (!res.ok && res.status !== 206) {
      return null;
    }

    const cl = res.headers.get("content-length");
    if (cl) {
      const len = parseInt(cl, 10);
      if (!isNaN(len) && len > 0) {
        return len < ONE_MB;
      }
    }

    // Read chunked stream up to 1MB to check if it finishes before reaching 1MB
    if (res.body) {
      const reader = res.body.getReader();
      let bytesRead = 0;

      while (true) {
        const { done, value } = await reader.read();
        if (done) {
          return bytesRead < ONE_MB;
        }
        bytesRead += (value ? value.length : 0);
        if (bytesRead >= ONE_MB) {
          try {
            reader.cancel();
          } catch (e) {}
          return false;
        }
      }
    }

    return null;
  } catch (err) {
    clearTimeout(timeoutId);
    return null;
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

  // FEATURE: Do not download files < 1MB (1,048,576 bytes) - let native browser download engine handle them
  const ONE_MB = 1024 * 1024;
  if (fileSize > 0 && fileSize < ONE_MB) {
    console.log(`[IDM Extension] File size (${fileSize} bytes) < 1MB. Letting native browser download engine handle: ${filenameStr || targetRealUrl}`);
    return false;
  }

  // Edge Case: If fileSize is unknown (0 or -1, e.g., chunked streams like Google Docs / Drive exports)
  if (fileSize <= 0) {
    const cookies = await getCookiesForUrl(targetRealUrl);
    const isSmall = await checkUnknownSizeIsSmall(targetRealUrl, cookies);
    if (isSmall === true) {
      console.log(`[IDM Extension] Verified chunked stream is < 1MB (e.g. Google Docs/Sheets export). Letting native browser download engine handle: ${filenameStr || targetRealUrl}`);
      return false;
    }
  }

  const cleanFilename = filenameStr || extractFilename(targetRealUrl);

  const isBinaryExt =
    /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage|7z|rar|bz2|xz|pdf|docx?|xlsx?|pptx?|mp4|mkv|webm|mp3|wav|flac|bin|pkg|csv|epub|mobi)$/i.test(cleanFilename) ||
    /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage|7z|rar|bz2|xz|pdf|docx?|xlsx?|pptx?|mp4|mkv|webm|mp3|wav|flac|bin|pkg|csv|epub|mobi)$/i.test(url) ||
    /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage|7z|rar|bz2|xz|pdf|docx?|xlsx?|pptx?|mp4|mkv|webm|mp3|wav|flac|bin|pkg|csv|epub|mobi)$/i.test(targetRealUrl);
  const isNonTextMime = mime && !mime.startsWith("text/");
  const hasSubstantialSize = fileSize >= ONE_MB;

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

    const cookies = await getCookiesForUrl(targetRealUrl);
    let referrer = item.referrer || "";
    if (!referrer) {
      try {
        const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
        if (tabs && tabs[0]) {
          referrer = tabs[0].url || "";
        }
      } catch (e) {}
    }

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
