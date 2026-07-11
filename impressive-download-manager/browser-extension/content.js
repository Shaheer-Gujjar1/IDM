// Content script to sniff media assets on web pages
const mediaExtensions = ['mp4', 'mkv', 'webm', 'mp3', 'wav', 'pdf', 'zip', 'rar', '7z', 'tar', 'gz'];
const streamExtensions = ['m3u8', 'mpd'];

function sniffMedia() {
  const mediaUrls = new Set();

  // 1. Sniff video & audio tags
  const mediaElements = document.querySelectorAll('video, audio, source');
  mediaElements.forEach((el) => {
    let src = '';
    if (el.src) {
      src = el.src;
    } else if (el.getAttribute('src')) {
      src = el.getAttribute('src');
    }
    
    if (src && src.startsWith('http')) {
      mediaUrls.add(src);
    }
  });

  // 2. Sniff anchor download links
  const links = document.querySelectorAll('a');
  links.forEach((a) => {
    const href = a.href;
    if (href && href.startsWith('http')) {
      try {
        const urlObj = new URL(href);
        const ext = urlObj.pathname.split('.').pop()?.toLowerCase();
        if (ext && (mediaExtensions.includes(ext) || streamExtensions.includes(ext))) {
          mediaUrls.add(href);
        }
      } catch (e) {}
    }
  });

  // Report all sniffed URLs to background service worker
  if (mediaUrls.size > 0) {
    chrome.runtime.sendMessage({
      type: 'MEDIA_DETECTED',
      urls: Array.from(mediaUrls),
      title: document.title
    }).catch(() => {});
  }
}

// Perform initial check
window.addEventListener('DOMContentLoaded', sniffMedia);
// Re-check periodically for dynamic media content
setInterval(sniffMedia, 3000);

// Intercept direct download link clicks to bypass browser picker dialogue
document.addEventListener('click', (e) => {
  const anchor = e.target.closest('a');
  if (anchor && anchor.href && anchor.href.startsWith('http')) {
    try {
      const urlObj = new URL(anchor.href);
      const ext = urlObj.pathname.split('.').pop()?.toLowerCase();
      const downloadExtensions = [
        'zip', 'rar', '7z', 'tar', 'gz', 'exe', 'msi', 'dmg', 
        'pkg', 'iso', 'pdf', 'apk', 'crx', 'docx', 'xlsx', 'pptx'
      ];
      if (ext && downloadExtensions.includes(ext)) {
        // Stop default click/download navigation
        e.preventDefault();
        e.stopPropagation();

        chrome.runtime.sendMessage({
          type: 'DIRECT_DOWNLOAD',
          url: anchor.href,
          filename: anchor.download || urlObj.pathname.split('/').pop() || 'download_file'
        }).catch(() => {});
      }
    } catch (err) {}
  }
}, true); // Use capture phase

