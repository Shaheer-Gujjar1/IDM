// Service worker for Impressive Download Manager Extension
const PORT = 9600;

// Helper to extract cookie headers for a target URL
async function getCookiesForUrl(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map(c => `${c.name}=${c.value}`).join('; ');
  } catch (err) {
    console.warn("Failed to get cookies:", err);
    return "";
  }
}

// Intercept browser downloads automatically (Fallback)
chrome.downloads.onDeterminingFilename.addListener((downloadItem, suggest) => {
  // Only capture active downloads
  if (downloadItem.state && downloadItem.state !== "in_progress") {
    suggest();
    return;
  }

  const url = downloadItem.url || "";
  const referrer = downloadItem.referrer || "";
  if (url.includes("web.whatsapp.com") || referrer.includes("web.whatsapp.com")) {
    suggest();
    return;
  }

  // Whitelist image downloads (MIME type or file extension)
  const filenameStr = downloadItem.filename || "";
  const mime = downloadItem.mime || "";
  const isImage = mime.startsWith("image/") || 
                  /\.(png|jpe?g|gif|webp|svg|bmp|ico|tiff?)(?:\?.*)?$/i.test(url) ||
                  /\.(png|jpe?g|gif|webp|svg|bmp|ico|tiff?)$/i.test(filenameStr);
  if (isImage) {
    suggest();
    return;
  }

  // Do not capture downloads <= 1.00MB (1,048,576 bytes)
  // If fileSize is -1 or 0, it means size is unknown, so we capture it.
  if (downloadItem.fileSize > 0 && downloadItem.fileSize <= 1048576) {
    suggest();
    return;
  }

  // Ignore historical/restored downloads loaded by Chrome at startup
  if (downloadItem.startTime) {
    const downloadTime = new Date(downloadItem.startTime).getTime();
    const now = Date.now();
    if (now - downloadTime > 10000) {
      suggest();
      return;
    }
  }

  if (downloadItem.url && !downloadItem.byExtensionId) {
    // Cancel download immediately to prevent browser's save dialogue from showing up
    chrome.downloads.cancel(downloadItem.id);
    suggest();

    const filename = downloadItem.filename 
      ? downloadItem.filename.split(/[\\/]/).pop() 
      : extractFilename(downloadItem.url);
    
    (async () => {
      const cookies = await getCookiesForUrl(downloadItem.url);
      let referrer = downloadItem.referrer || "";
      if (!referrer) {
        try {
          const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
          if (tabs && tabs[0]) {
            referrer = tabs[0].url || "";
          }
        } catch (err) {
          console.warn("Failed to fetch fallback tab referrer:", err);
        }
      }
      await sendToDesktopApp(downloadItem.url, filename, cookies, referrer);
    })();
  } else {
    suggest();
  }
});

// Listen to messages from content scripts (media sniffing)
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === 'MEDIA_DETECTED') {
    (async () => {
      const tabId = sender.tab?.id;
      if (!tabId) return;

      const storeKey = `captured_tab_${tabId}`;
      const data = await chrome.storage.local.get(storeKey);
      const existing = data[storeKey] || [];

      const updated = [...existing];
      message.urls.forEach(url => {
        if (!updated.some(item => item.url === url)) {
          updated.push({
            url,
            filename: extractFilename(url),
            timestamp: Date.now()
          });
        }
      });

      await chrome.storage.local.set({ [storeKey]: updated });
      
      if (updated.length > 0) {
        await chrome.action.setBadgeText({
          tabId: tabId,
          text: String(updated.length)
        });
        await chrome.action.setBadgeBackgroundColor({
          tabId: tabId,
          color: "#00f0ff"
        });
      }
    })();
  }
  return true;
});

const recentInterceptions = new Map();

// Helper to send payloads to our Tauri backend port 9600
async function sendToDesktopApp(url, filename, cookie = "", referrer = "") {
  const now = Date.now();
  const lastIntercept = recentInterceptions.get(url);
  if (lastIntercept && (now - lastIntercept) < 6000) {
    console.log("De-duplicated download popup trigger for URL:", url);
    return false;
  }
  recentInterceptions.set(url, now);

  // Periodic cleanup
  for (const [key, time] of recentInterceptions.entries()) {
    if (now - time > 15000) {
      recentInterceptions.delete(key);
    }
  }

  try {
    const response = await fetch(`http://127.0.0.1:${PORT}/download`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      mode: 'cors',
      body: JSON.stringify({ url, filename, cookie, referrer })
    });
    const result = await response.json();
    console.log("Desktop app response:", result);
    return true;
  } catch (err) {
    console.error("Failed to connect to Impressive Download Manager app:", err);
    return false;
  }
}

// Extractor helper
function extractFilename(url) {
  try {
    const pathname = new URL(url).pathname;
    const last = pathname.substring(pathname.lastIndexOf("/") + 1);
    return last || "captured_download";
  } catch {
    return "captured_download";
  }
}

// Export sending logic for popup
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === 'SEND_TO_DESKTOP') {
    (async () => {
      const cookies = await getCookiesForUrl(message.url);
      const referrer = "";
      const success = await sendToDesktopApp(message.url, message.filename, cookies, referrer);
      sendResponse({ success });
    })();
    return true;
  }
});
