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

  function scan() {
    pending = false;
    if (!connected()) return;

    for (const video of document.querySelectorAll('video')) {
      if (seen.has(video)) continue;
      seen.add(video);

      const host = document.createElement('div');
      host.style.cssText = 'position:fixed;z-index:2147483647;display:none;pointer-events:auto';

      // Closed Shadow DOM ensures host page scripts/styles can NEVER break or conflict with this widget
      const shadow = host.attachShadow({ mode: 'closed' });
      shadow.innerHTML = `
        <style>
          :host {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            font-size: 12px;
            color: #ffffff;
            user-select: none;
            -webkit-user-select: none;
          }
          .bar {
            display: flex;
            align-items: stretch;
            box-shadow: 0 4px 14px rgba(0,0,0,0.4);
            border-radius: 6px;
            overflow: hidden;
            background: linear-gradient(135deg, #1e293b, #0f172a);
            border: 1px solid rgba(255,255,255,0.15);
            transition: transform 0.15s ease, box-shadow 0.15s ease;
          }
          .bar:hover {
            transform: translateY(-1px);
            box-shadow: 0 6px 20px rgba(0,0,0,0.55);
            border-color: rgba(59,130,246,0.6);
          }
          .download, .dismiss {
            background: transparent;
            color: #f1f5f9;
            border: 0;
            cursor: pointer;
            height: 28px;
            display: inline-flex;
            align-items: center;
            font-size: 12px;
            font-weight: 500;
          }
          .download {
            padding: 0 10px;
            gap: 6px;
          }
          .download:hover {
            background: rgba(59,130,246,0.25);
            color: #60a5fa;
          }
          .dismiss {
            width: 26px;
            justify-content: center;
            border-left: 1px solid rgba(255,255,255,0.1);
            font-size: 15px;
            color: #94a3b8;
          }
          .dismiss:hover {
            background: rgba(239,68,68,0.25);
            color: #f87171;
          }
          .chooser {
            position: fixed;
            background: #0f172a;
            color: #f8fafc;
            padding: 6px;
            border: 1px solid rgba(255,255,255,0.15);
            border-radius: 8px;
            width: min(420px, calc(100vw - 12px));
            box-shadow: 0 10px 25px rgba(0,0,0,0.65);
            max-height: min(340px, 55vh);
            overflow-y: auto;
            backdrop-filter: blur(12px);
            -webkit-backdrop-filter: blur(12px);
          }
          .hint {
            margin: 6px 8px 8px;
            font-size: 11px;
            color: #94a3b8;
            font-weight: 500;
          }
          .row {
            display: flex;
            align-items: center;
            justify-content: space-between;
            width: 100%;
            text-align: left;
            background: transparent;
            border: 0;
            border-radius: 5px;
            padding: 6px 10px;
            font-size: 12px;
            color: #e2e8f0;
            cursor: pointer;
            transition: background 0.12s ease;
          }
          .row:hover:not(:disabled) {
            background: #1e293b;
            color: #38bdf8;
          }
          .row:disabled {
            opacity: 0.5;
            cursor: not-allowed;
          }
          .badge {
            font-size: 10px;
            padding: 2px 6px;
            border-radius: 4px;
            background: rgba(59,130,246,0.2);
            color: #60a5fa;
            font-weight: 600;
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
        hint.className = 'hint';
        hint.textContent = 'Scanning available video qualities…';
        chooser.appendChild(hint);
        shadow.appendChild(chooser);
        position();

        try {
          const result = await send({ action: 'listForFrame' });
          if (!result?.ok) throw new Error(result?.error || 'Could not list video formats.');

          hint.textContent = 'Select resolution to download';
          const candidates = result?.items || [];
          const videoSrc = direct(video.currentSrc || video.src || video.querySelector('source')?.src);
          if (videoSrc && !candidates.some(c => c.url === videoSrc)) {
            candidates.unshift({ url: videoSrc, title: document.title });
          }

          if (!candidates.length) {
            hint.textContent = 'No media links captured yet. Play video for 2 seconds and try again.';
            return;
          }

          const choices = globalThis.mediaChoices ? globalThis.mediaChoices(candidates, { url: videoSrc, poster: video.poster }) : candidates;
          if (!choices.length) {
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
            badge.textContent = format;

            row.appendChild(left);
            row.appendChild(badge);

            row.onclick = async ev => {
              ev.preventDefault();
              ev.stopPropagation();
              row.disabled = true;
              hint.textContent = 'Sending download to Impressive Download Manager…';

              try {
                const response = await send({
                  action: 'download',
                  item: { ...item, filename: `${document.title.replace(/[<>:"/\\|?*]/g, '_') || 'video'}.${item.outputFormat || 'mp4'}` }
                });
                if (response?.ok) {
                  hint.textContent = '✓ Download captured in Impressive Download Manager!';
                  setTimeout(() => {
                    chooser.remove();
                    position();
                  }, 1200);
                } else {
                  hint.textContent = response?.error || 'Failed to trigger download.';
                }
              } catch (err) {
                hint.textContent = err.message;
              }
              row.disabled = false;
            };

            chooser.appendChild(row);
          });
          position();
        } catch (err) {
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
      const height = record.host.offsetHeight || 28;
      const above = rect.top >= height + 8;

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

  function onMessage(message, sender, respond) {
    if (!connected() || sender.id !== api.runtime.id) return;
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
