(() => {
  const api = globalThis.browser || chrome;
  const seen = new WeakSet();
  const records = new Set();
  let pending = false;
  let stopped = false, scanFrame, mutationObserver, scrollTimer, scrolling = false;
  const listeners = new AbortController();
  const resizeObserver = new ResizeObserver(() => position());

  const direct = value => {
    try {
      const u = new URL(value, location.href);
      return /^https?:$/.test(u.protocol) ? u.href : null;
    } catch {
      return null;
    }
  };

  function shutdown() {
    if (stopped) return;
    stopped = true;
    listeners.abort();
    clearTimeout(scrollTimer);
    mutationObserver?.disconnect();
    resizeObserver.disconnect();
    if (scanFrame !== undefined) cancelAnimationFrame(scanFrame);
    for (const record of records) {
      record.sourceObserver?.disconnect();
      record.host.remove();
    }
    records.clear();
    try {
      api.runtime.onMessage.removeListener(onMessage);
    } catch {}
  }

  function connected() {
    if (stopped) return false;
    try {
      if (api.runtime.id) return true;
    } catch {}
    shutdown();
    return false;
  }

  async function send(message) {
    if (!connected()) throw new Error('The extension was reloaded. Refresh this page to reconnect.');
    try {
      return await api.runtime.sendMessage(message);
    } catch (error) {
      if (/extension context invalidated/i.test(error?.message || '')) shutdown();
      throw error;
    }
  }

  let currentAppTheme = 'dark';

  function resolveTheme(mode) {
    if (mode === 'light') return 'light';
    if (mode === 'dark') return 'dark';
    const isSystemDark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
    return isSystemDark ? 'dark' : 'light';
  }

  function detectPreferredTheme() {
    return resolveTheme(currentAppTheme);
  }

  function applyTheme(newMode) {
    if (newMode) currentAppTheme = newMode;
    const resolved = resolveTheme(currentAppTheme);
    for (const record of records) {
      record.host.setAttribute('data-theme', resolved);
    }
  }

  try {
    api.storage?.local?.get('appTheme', (data) => {
      if (data?.appTheme) applyTheme(data.appTheme);
    });
    api.runtime?.sendMessage({ action: 'getTheme' }, (res) => {
      if (res?.theme) applyTheme(res.theme);
    });
  } catch {}

  api.storage?.onChanged?.addListener((changes, area) => {
    if (area === 'local' && changes.appTheme) {
      applyTheme(changes.appTheme.newValue);
    }
  });

  function scan() {
    pending = false;
    if (!connected()) return;

    for (const video of document.querySelectorAll('video')) {
      if (seen.has(video)) continue;
      seen.add(video);

      const host = document.createElement('div');
      host.style.cssText = 'position:fixed;z-index:2147483647;display:none;pointer-events:auto';
      host.setAttribute('data-theme', resolveTheme(currentAppTheme));

      // Closed Shadow DOM ensures host page scripts/styles can NEVER break or conflict with this widget
      const shadow = host.attachShadow({ mode: 'closed' });
      shadow.innerHTML = `
        <style>
          :host {
            font-family: 'Outfit', -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
            font-size: 12px;
            user-select: none;
            -webkit-user-select: none;
            
            /* IDM App Dark Theme (Identical to Download Captures & Progress Popups) */
            --idm-bg-bar: linear-gradient(135deg, rgba(10, 12, 19, 0.94), rgba(18, 24, 38, 0.94));
            --idm-border-bar: rgba(255, 255, 255, 0.12);
            --idm-border-bar-hover: rgba(6, 182, 212, 0.6);
            --idm-shadow-bar: 0 8px 24px rgba(0, 0, 0, 0.55), 0 0 0 1px rgba(255, 255, 255, 0.08);
            --idm-shadow-bar-hover: 0 12px 32px rgba(0, 0, 0, 0.75), 0 0 0 1.5px rgba(6, 182, 212, 0.5), 0 0 20px rgba(6, 182, 212, 0.2);
            
            --idm-text-primary: #ffffff;
            --idm-text-secondary: #a1a1aa;
            --idm-text-hover: #06b6d4;
            
            --idm-btn-hover-bg: rgba(6, 182, 212, 0.15);
            --idm-icon-color: #06b6d4;
            
            --idm-dismiss-hover-bg: rgba(239, 68, 68, 0.22);
            --idm-dismiss-hover-color: #f87171;
            --idm-divider: rgba(255, 255, 255, 0.12);
            
            --idm-chooser-bg: rgba(10, 14, 23, 0.96);
            --idm-chooser-border: rgba(255, 255, 255, 0.12);
            --idm-chooser-shadow: 0 20px 50px rgba(0, 0, 0, 0.7), 0 0 0 1px rgba(255, 255, 255, 0.08);
            
            --idm-hint-bg: rgba(255, 255, 255, 0.04);
            --idm-hint-border: rgba(255, 255, 255, 0.08);
            
            --idm-row-hover-bg: rgba(6, 182, 212, 0.12);
            --idm-row-hover-border: rgba(6, 182, 212, 0.35);
            --idm-row-hover-text: #ffffff;
            
            --idm-badge-bg: rgba(6, 182, 212, 0.18);
            --idm-badge-text: #22d3ee;
            --idm-badge-border: rgba(6, 182, 212, 0.35);
            
            --idm-scroll-thumb: rgba(255, 255, 255, 0.2);
          }

          :host([data-theme="light"]) {
            /* IDM App Light Theme (Identical to Download Captures & Progress Popups) */
            --idm-bg-bar: linear-gradient(135deg, rgba(255, 255, 255, 0.97), rgba(241, 245, 249, 0.97));
            --idm-border-bar: rgba(0, 0, 0, 0.1);
            --idm-border-bar-hover: rgba(2, 132, 199, 0.55);
            --idm-shadow-bar: 0 8px 24px rgba(15, 23, 42, 0.12), 0 0 0 1px rgba(0, 0, 0, 0.06);
            --idm-shadow-bar-hover: 0 12px 32px rgba(15, 23, 42, 0.18), 0 0 0 1.5px rgba(2, 132, 199, 0.45), 0 0 16px rgba(2, 132, 199, 0.15);
            
            --idm-text-primary: #0f172a;
            --idm-text-secondary: #475569;
            --idm-text-hover: #0284c7;
            
            --idm-btn-hover-bg: rgba(2, 132, 199, 0.1);
            --idm-icon-color: #0284c7;
            
            --idm-dismiss-hover-bg: rgba(239, 68, 68, 0.12);
            --idm-dismiss-hover-color: #dc2626;
            --idm-divider: rgba(0, 0, 0, 0.08);
            
            --idm-chooser-bg: rgba(255, 255, 255, 0.98);
            --idm-chooser-border: rgba(0, 0, 0, 0.1);
            --idm-chooser-shadow: 0 20px 50px rgba(15, 23, 42, 0.15), 0 0 0 1px rgba(0, 0, 0, 0.08);
            
            --idm-hint-bg: rgba(15, 23, 42, 0.04);
            --idm-hint-border: rgba(15, 23, 42, 0.08);
            
            --idm-row-hover-bg: rgba(2, 132, 199, 0.08);
            --idm-row-hover-border: rgba(2, 132, 199, 0.3);
            --idm-row-hover-text: #0f172a;
            
            --idm-badge-bg: rgba(2, 132, 199, 0.12);
            --idm-badge-text: #0284c7;
            --idm-badge-border: rgba(2, 132, 199, 0.3);
            
            --idm-scroll-thumb: rgba(15, 23, 42, 0.2);
          }

          .bar {
            display: flex;
            align-items: center;
            border-radius: 9999px;
            box-shadow: var(--idm-shadow-bar);
            background: var(--idm-bg-bar);
            border: 1px solid var(--idm-border-bar);
            backdrop-filter: blur(16px) saturate(180%);
            -webkit-backdrop-filter: blur(16px) saturate(180%);
            transition: transform 0.2s cubic-bezier(0.16, 1, 0.3, 1), box-shadow 0.2s cubic-bezier(0.16, 1, 0.3, 1), border-color 0.2s ease;
            overflow: hidden;
            height: 30px;
          }
          .bar:hover {
            transform: translateY(-1px) scale(1.02);
            box-shadow: var(--idm-shadow-bar-hover);
            border-color: var(--idm-border-bar-hover);
          }
          .download, .dismiss {
            background: transparent;
            color: var(--idm-text-primary);
            border: 0;
            cursor: pointer;
            height: 100%;
            display: inline-flex;
            align-items: center;
            font-size: 12px;
            font-weight: 600;
            letter-spacing: -0.01em;
            transition: background 0.15s ease, color 0.15s ease;
          }
          .download {
            padding: 0 12px;
            gap: 7px;
          }
          .download svg {
            color: var(--idm-icon-color);
            flex-shrink: 0;
            transition: transform 0.2s ease;
          }
          .download:hover {
            background: var(--idm-btn-hover-bg);
            color: var(--idm-text-hover);
          }
          .download:hover svg {
            transform: translateY(1px);
          }
          .dismiss {
            width: 28px;
            justify-content: center;
            border-left: 1px solid var(--idm-divider);
            font-size: 16px;
            color: var(--idm-text-secondary);
            line-height: 1;
          }
          .dismiss:hover {
            background: var(--idm-dismiss-hover-bg);
            color: var(--idm-dismiss-hover-color);
          }
          .chooser {
            position: fixed;
            background: var(--idm-chooser-bg);
            color: var(--idm-text-primary);
            padding: 8px;
            border: 1px solid var(--idm-chooser-border);
            border-radius: 12px;
            width: min(440px, calc(100vw - 16px));
            box-shadow: var(--idm-chooser-shadow);
            max-height: min(360px, 60vh);
            overflow-y: auto;
            backdrop-filter: blur(20px) saturate(180%);
            -webkit-backdrop-filter: blur(20px) saturate(180%);
            animation: idm-pop-in 0.18s cubic-bezier(0.16, 1, 0.3, 1);
            scrollbar-width: thin;
            scrollbar-color: var(--idm-scroll-thumb) transparent;
          }
          .chooser::-webkit-scrollbar {
            width: 5px;
          }
          .chooser::-webkit-scrollbar-thumb {
            background: var(--idm-scroll-thumb);
            border-radius: 6px;
          }
          @keyframes idm-pop-in {
            from { opacity: 0; transform: translateY(-4px) scale(0.97); }
            to { opacity: 1; transform: translateY(0) scale(1); }
          }
          .hint {
            margin: 4px 4px 8px;
            padding: 6px 10px;
            background: var(--idm-hint-bg);
            border: 1px solid var(--idm-hint-border);
            border-radius: 7px;
            font-size: 11.5px;
            color: var(--idm-text-secondary);
            font-weight: 500;
            line-height: 1.4;
            transition: all 0.2s ease;
          }
          .hint.scanning {
            color: var(--idm-icon-color);
            animation: idm-pulse 1.4s ease-in-out infinite;
          }
          .hint.success {
            color: #10b981;
            font-weight: 600;
          }
          .hint.error {
            color: #ef4444;
          }
          @keyframes idm-pulse {
            0%, 100% { opacity: 1; }
            50% { opacity: 0.55; }
          }
          .row {
            display: flex;
            align-items: center;
            justify-content: space-between;
            width: 100%;
            text-align: left;
            background: transparent;
            border: 1px solid transparent;
            border-radius: 8px;
            padding: 8px 10px;
            margin-bottom: 3px;
            font-size: 12px;
            color: var(--idm-text-primary);
            cursor: pointer;
            transition: all 0.14s cubic-bezier(0.16, 1, 0.3, 1);
            box-sizing: border-box;
          }
          .row:hover:not(:disabled) {
            background: var(--idm-row-hover-bg);
            color: var(--idm-row-hover-text);
            border-color: var(--idm-row-hover-border);
            transform: translateX(2px);
          }
          .row:disabled {
            opacity: 0.5;
            cursor: not-allowed;
          }
          .badge {
            font-size: 10px;
            padding: 2.5px 7px;
            border-radius: 6px;
            background: var(--idm-badge-bg);
            color: var(--idm-badge-text);
            border: 1px solid var(--idm-badge-border);
            font-weight: 700;
            letter-spacing: 0.02em;
            flex-shrink: 0;
            margin-left: 8px;
          }
          .badge-4k {
            background: rgba(168, 85, 247, 0.2);
            color: #c084fc;
            border-color: rgba(168, 85, 247, 0.4);
          }
          .badge-1080p {
            background: rgba(6, 182, 212, 0.2);
            color: #22d3ee;
            border-color: rgba(6, 182, 212, 0.4);
          }
          .badge-720p {
            background: rgba(59, 130, 246, 0.2);
            color: #60a5fa;
            border-color: rgba(59, 130, 246, 0.4);
          }
          .badge-audio {
            background: rgba(245, 158, 11, 0.2);
            color: #fbbf24;
            border-color: rgba(245, 158, 11, 0.4);
          }
        </style>
        <div class="bar">
          <button class="download" type="button">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
              <polyline points="7 10 12 15 17 10"></polyline>
              <line x1="12" y1="15" x2="12" y2="3"></line>
            </svg>
            Download video ▾
          </button>
          <button class="dismiss" type="button" title="Hide video download bar" aria-label="Hide video download bar">×</button>
        </div>
      `;

      const button = shadow.querySelector('.download');
      const dismiss = shadow.querySelector('.dismiss');
      const record = { video, host, shadow, dismissed: false };
      records.add(record);

      resizeObserver.observe(video);
      document.documentElement.appendChild(host);

      const report = () => {
        const urls = [video.currentSrc, video.src, ...Array.from(video.querySelectorAll('source'), s => s.src)].map(direct).filter(Boolean);
        if (urls.length) {
          send({
            action: 'remember',
            items: urls.map(url => ({ url, title: document.title }))
          }).catch(() => {});
        }
      };

      video.addEventListener('loadedmetadata', report, { signal: listeners.signal });
      record.sourceObserver = new MutationObserver(report);
      record.sourceObserver.observe(video, { attributes: true, attributeFilter: ['src'], subtree: true, childList: true });
      report();
      if (stopped) return;

      dismiss.addEventListener('click', e => {
        e.preventDefault();
        e.stopPropagation();
        record.dismissed = true;
        shadow.querySelector('.chooser')?.remove();
        host.style.display = 'none';
      });

      button.addEventListener('click', async e => {
        e.preventDefault();
        e.stopPropagation();

        const old = shadow.querySelector('.chooser');
        if (old) {
          old.remove();
          position();
          return;
        }

        closeChoosers(record);
        const chooser = document.createElement('div');
        chooser.className = 'chooser';

        const hint = document.createElement('div');
        hint.className = 'hint scanning';
        hint.textContent = 'Scanning available video qualities…';
        chooser.appendChild(hint);
        shadow.appendChild(chooser);
        position();

        try {
          if (location.hostname.includes('youtube.com')) {
            window.dispatchEvent(new CustomEvent('impressive-request-player'));
            await new Promise(r => setTimeout(r, 120));
          }
          const result = await send({ action: 'listForFrame' });
          if (!result?.ok) throw new Error(result?.error || 'Could not list video formats.');

          hint.className = 'hint';
          hint.textContent = 'Select resolution to download';
          const candidates = result?.items || [];
          const videoSrc = direct(video.currentSrc || video.src || video.querySelector('source')?.src);
          if (videoSrc && !candidates.some(c => c.url === videoSrc)) {
            candidates.unshift({ url: videoSrc, title: document.title });
          }

          if (!candidates.length) {
            hint.className = 'hint';
            hint.textContent = 'No media links captured yet. Play video for 2 seconds and try again.';
            return;
          }

          const choices = globalThis.mediaChoices ? globalThis.mediaChoices(candidates, { url: videoSrc, poster: video.poster }) : candidates;
          if (!choices.length) {
            hint.className = 'hint';
            hint.textContent = 'No complete formats detected. Play video briefly and reopen.';
            return;
          }

          choices.forEach((item, idx) => {
            const row = document.createElement('button');
            row.className = 'row';
            row.type = 'button';

            const left = document.createElement('span');
            const format = item.format || (/\.m3u8(?:[?#]|$)/i.test(item.url) ? 'HLS' : 'MP4');
            const quality = item.quality || format;
            left.textContent = `${idx + 1}. ${quality} · ${item.audioState || 'Complete video'}`;

            const badge = document.createElement('span');
            badge.className = 'badge';
            const qLower = (quality || '').toLowerCase();
            if (qLower.includes('4k') || qLower.includes('2160')) {
              badge.className = 'badge badge-4k';
            } else if (qLower.includes('1080')) {
              badge.className = 'badge badge-1080p';
            } else if (qLower.includes('720')) {
              badge.className = 'badge badge-720p';
            } else if (qLower.includes('audio')) {
              badge.className = 'badge badge-audio';
            }
            badge.textContent = format;

            row.appendChild(left);
            row.appendChild(badge);

            row.onclick = async ev => {
              ev.preventDefault();
              ev.stopPropagation();
              row.disabled = true;
              hint.className = 'hint scanning';
              hint.textContent = 'Sending download to Impressive Download Manager…';

              try {
                const response = await send({
                  action: 'download',
                  item: {
                    ...item,
                    pageUrl: item.pageUrl || window.location.href,
                    referrer: window.location.href,
                    filename: item.filename || `${document.title.replace(/[<>:"/\\|?*]/g, '_') || 'video'}.${item.outputFormat || 'mp4'}`
                  }
                });
                if (response?.ok) {
                  hint.className = 'hint success';
                  hint.textContent = '✓ Download captured in Impressive Download Manager!';
                  setTimeout(() => {
                    chooser.remove();
                    position();
                  }, 1200);
                } else {
                  hint.className = 'hint error';
                  hint.textContent = response?.error || 'Failed to trigger download.';
                }
              } catch (err) {
                hint.className = 'hint error';
                hint.textContent = err.message;
              }
              row.disabled = false;
            };

            chooser.appendChild(row);
          });
          position();
        } catch (err) {
          hint.className = 'hint error';
          hint.textContent = err.message;
        }
      });
    }

    position();
  }

  function closeChoosers(except) {
    for (const record of records) {
      if (record === except) continue;
      const ch = record.shadow.querySelector('.chooser');
      if (ch) ch.remove();
    }
    position();
  }

  function position() {
    if (!connected()) return;
    for (const record of records) {
      if (!record.video.isConnected) {
        resizeObserver.unobserve(record.video);
        record.sourceObserver?.disconnect();
        record.host.remove();
        records.delete(record);
        continue;
      }

      const fullscreen = document.fullscreenElement;
      const parent = fullscreen && fullscreen !== record.video && fullscreen.contains(record.video) ? fullscreen : document.documentElement;
      if (record.host.parentNode !== parent) parent.appendChild(record.host);

      const rect = record.video.getBoundingClientRect();
      const visible = !scrolling && !record.dismissed && rect.width > 120 && rect.height > 80 && rect.top >= 0 && rect.top < innerHeight - 20 && rect.right > 0 && rect.left < innerWidth;
      record.host.style.display = visible ? 'block' : 'none';
      if (!visible) continue;

      const width = record.host.offsetWidth || 160;
      const height = record.host.offsetHeight || 30;
      const above = rect.top >= height + 8;

      record.host.setAttribute('data-theme', detectPreferredTheme());
      record.host.style.top = `${Math.max(6, Math.min(innerHeight - height - 6, above ? rect.top - height - 6 : rect.top + 6))}px`;
      record.host.style.left = `${Math.max(6, Math.min(innerWidth - width - 8, rect.right - width - 10))}px`;

      const chooser = record.shadow.querySelector('.chooser');
      if (chooser) {
        const chooserWidth = Math.min(420, innerWidth - 12);
        const chooserHeight = Math.min(chooser.scrollHeight || 300, Math.min(320, innerHeight * 0.55));
        const hostRect = record.host.getBoundingClientRect();
        chooser.style.left = `${Math.max(6, Math.min(innerWidth - chooserWidth - 6, hostRect.right - chooserWidth))}px`;
        const below = hostRect.bottom + 6;
        const aboveMenu = hostRect.top - chooserHeight - 6;
        chooser.style.top = `${below + chooserHeight <= innerHeight - 6 ? below : Math.max(6, aboveMenu)}px`;
      }
    }
  }

  mutationObserver = new MutationObserver(() => {
    if (!stopped && !pending) {
      pending = true;
      scanFrame = requestAnimationFrame(scan);
    }
  });
  mutationObserver.observe(document.documentElement, { subtree: true, childList: true });

  document.addEventListener('pointerdown', event => {
    const path = event.composedPath();
    const inside = [...records].find(r => path.includes(r.host));
    if (!inside) closeChoosers();
  }, { capture: true, signal: listeners.signal });

  document.addEventListener('keydown', event => {
    if (event.key === 'Escape') closeChoosers();
  }, { capture: true, signal: listeners.signal });

  function onScroll(event) {
    if (event?.composedPath?.().some(node => [...records].some(r => node === r.host))) return;
    scrolling = true;
    closeChoosers();
    clearTimeout(scrollTimer);
    scrollTimer = setTimeout(() => {
      scrolling = false;
      position();
    }, 120);
  }

  addEventListener('scroll', onScroll, { capture: true, signal: listeners.signal });
  addEventListener('resize', position, { signal: listeners.signal });
  document.addEventListener('fullscreenchange', position, { signal: listeners.signal });

  if (window.matchMedia) {
    try {
      window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
        if (currentAppTheme === 'system') {
          applyTheme('system');
        }
      }, { signal: listeners.signal });
    } catch (e) {}
  }

  function onMessage(message, sender, respond) {
    if (!connected() || sender.id !== api.runtime.id) return;
    if (message.action === 'themeChanged' && message.theme) {
      applyTheme(message.theme);
      try { respond({ ok: true }); } catch {}
      return true;
    }
    if (message.action === 'collectLinks') {
      const items = Array.from(document.querySelectorAll('a[href]'))
        .slice(0, 500)
        .map(a => ({ url: direct(a.href), title: a.textContent.trim() }))
        .filter(item => item.url);
      send({ action: 'remember', items: items.slice(0, 100) })
        .then(respond, err => { try { respond({ ok: false, error: err.message }); } catch {} })
        .catch(() => {});
      return true;
    }
  }

  api.runtime.onMessage.addListener(onMessage);
  scan();
})();
