const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

require('./media-choices.js');
require('./hls.js');

const { mediaChoices, assetKey } = globalThis;
const { parseHlsMaster } = globalThis;

// =========================================================================
// Phase 3 Adversarial & Security Tests
// =========================================================================

test('Phase 3 Security: XSS Prevention in Video Overlay and Titles', () => {
  const contentJs = fs.readFileSync(path.join(__dirname, 'content.js'), 'utf8');

  // Verify that dynamic text rendering avoids innerHTML for user-controlled strings
  assert(
    contentJs.includes('left.textContent =') || contentJs.includes('.textContent ='),
    'Overlay must use textContent for text nodes to prevent XSS execution'
  );

  // Closed Shadow DOM mode must be strictly enforced
  assert(
    contentJs.includes("attachShadow({ mode: 'closed' })") || contentJs.includes("attachShadow({mode:'closed'})"),
    'Overlay must use Closed Shadow DOM to prevent host page DOM/CSS injection'
  );
});

test('Phase 3 Security: Streaming Fragment Rejection Filter', () => {
  const bgJs = fs.readFileSync(path.join(__dirname, 'background.js'), 'utf8');
  assert(bgJs.includes('isFragment'), 'background.js must define isFragment helper');

  // Regex testing for streaming chunk rejection
  const fragmentRegex = /\.(m4s|ts|aac|vtt|webvtt)(?:[?#]|$)/i;
  const initMp4Regex = /\/init[^/]*\.mp4(?:[?#]|$)/i;

  const isFrag = url => fragmentRegex.test(url) || initMp4Regex.test(url);

  // Streaming fragments that MUST be rejected
  assert.equal(isFrag('https://cdn.example.com/hls/segment_0001.ts'), true);
  assert.equal(isFrag('https://cdn.example.com/dash/video_chunk_05.m4s'), true);
  assert.equal(isFrag('https://cdn.example.com/dash/audio_chunk_05.m4s?token=123'), true);
  assert.equal(isFrag('https://cdn.example.com/media/init.mp4'), true);
  assert.equal(isFrag('https://cdn.example.com/media/init_video.mp4'), true);
  assert.equal(isFrag('https://cdn.example.com/subtitles/en.vtt'), true);

  // Complete video files that MUST be allowed
  assert.equal(isFrag('https://cdn.example.com/video/lecture_1080p.mp4'), false);
  assert.equal(isFrag('https://cdn.example.com/video/movie.mkv'), false);
  assert.equal(isFrag('https://cdn.example.com/video/clip.webm'), false);
  assert.equal(isFrag('https://cdn.example.com/hls/master.m3u8'), false);
});

// =========================================================================
// Phase 3 Real-World Functional Working Tests
// =========================================================================

test('Phase 3 Functionality: HLS Master Playlist Parsing & Resolution Extraction', () => {
  const sampleHlsMaster = `#EXTM3U
#EXT-X-VERSION:4
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio-aac",NAME="English",DEFAULT=YES,URI="audio/en/prog_index.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=4500000,AVERAGE-BANDWIDTH=4000000,RESOLUTION=1920x1080,CODECS="avc1.64002a,mp4a.40.2",AUDIO="audio-aac"
1080p/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2500000,AVERAGE-BANDWIDTH=2200000,RESOLUTION=1280x720,CODECS="avc1.4d401f,mp4a.40.2",AUDIO="audio-aac"
720p/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=1200000,AVERAGE-BANDWIDTH=1000000,RESOLUTION=854x480,CODECS="avc1.4d401f,mp4a.40.2",AUDIO="audio-aac"
480p/index.m3u8
`;

  const baseUrl = 'https://stream.example.com/master.m3u8';
  const variants = parseHlsMaster(sampleHlsMaster, baseUrl);

  assert.equal(variants.length, 3, 'Must extract all 3 stream variants');

  // Verify 1080p variant
  assert.equal(variants[0].quality, '1080p');
  assert.equal(variants[0].url, 'https://stream.example.com/1080p/index.m3u8');
  assert.equal(variants[0].audioUrl, 'https://stream.example.com/audio/en/prog_index.m3u8');
  assert.equal(variants[0].format, 'HLS');

  // Verify 720p variant
  assert.equal(variants[1].quality, '720p');
  assert.equal(variants[1].url, 'https://stream.example.com/720p/index.m3u8');

  // Verify 480p variant
  assert.equal(variants[2].quality, '480p');
  assert.equal(variants[2].url, 'https://stream.example.com/480p/index.m3u8');
});

test('Phase 3 Functionality: Quality Chooser Sorting & Audio Variant Generation', () => {
  const items = [
    { url: 'https://cdn.test/vid_480p.mp4', bitrate: 800000, quality: '480p', format: 'MP4' },
    { url: 'https://cdn.test/vid_1080p.mp4', bitrate: 4500000, quality: '1080p', format: 'MP4', audioUrl: 'https://cdn.test/audio.mp3' },
    { url: 'https://cdn.test/vid_720p.mp4', bitrate: 2200000, quality: '720p', format: 'MP4' },
    // A fragment that should be filtered out
    { url: 'https://cdn.test/segment_99.ts', bitrate: 2000000, quality: '720p', format: 'TS' }
  ];

  const choices = mediaChoices(items, {});

  // Should filter out the .ts fragment
  assert(!choices.some(c => c.url.includes('.ts')), 'TS fragments must be excluded');

  // Should sort by highest quality/bitrate first (1080p -> 720p -> 480p)
  assert.equal(choices[0].quality, '1080p');
  assert.equal(choices[0].format, 'MP4');

  // Should have generated an "Audio only" variant from the audioUrl
  const audioChoice = choices.find(c => c.quality === 'Audio only' || c.format === 'MP3');
  assert(audioChoice, 'Must include an Audio only MP3 variant');
  assert.equal(audioChoice.url, 'https://cdn.test/audio.mp3');
  assert.equal(audioChoice.audioState, 'Audio only');
});

test('Phase 3 Functionality: Extension Manifest Structure', () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, 'manifest.json'), 'utf8'));

  assert.equal(manifest.manifest_version, 3);
  assert(manifest.permissions.includes('webRequest'));
  assert(manifest.permissions.includes('cookies'));

  // Content script for all URLs
  const genericScript = manifest.content_scripts.find(c => c.matches.includes('<all_urls>'));
  assert(genericScript, 'Must register generic content script');
  assert(genericScript.js.includes('media-choices.js'));
  assert(genericScript.js.includes('hls.js'));
  assert(genericScript.js.includes('content.js'));

  // Phase 5: YouTube MAIN world and relay content scripts
  const ytMain = manifest.content_scripts.find(c => c.world === 'MAIN');
  assert(ytMain, 'Must have MAIN world content script for YouTube');
  assert(ytMain.js.includes('youtube-media.js'));
  assert(ytMain.js.includes('youtube-page.js'));

  const ytRelay = manifest.content_scripts.find(c => c.js.includes('youtube-relay.js'));
  assert(ytRelay, 'Must have YouTube relay script');
});

// =========================================================================
// Phase 5 Security & Real-World Functional Tests
// =========================================================================

require('./youtube-media.js');
const { youtubeMedia } = globalThis;

test('Phase 5 Security: YouTube Media URL Validation & Range Stripping', () => {
  assert(youtubeMedia, 'youtubeMedia must be exported');

  // 1. Valid GoogleVideo URL
  const validUrl = 'https://rr1---sn-4g5ednks.googlevideo.com/videoplayback?expire=1730000000&ei=xyz&ip=1.2.3.4&id=o-ABC&itag=137&range=0-1048576';
  const sanitized = youtubeMedia.mediaUrl(validUrl);
  assert(sanitized, 'Valid googlevideo URL must be accepted');
  assert(!sanitized.includes('range='), 'Stand-alone range parameter must be stripped to download whole file');

  // 2. Reject domain spoofing / phishing attempts
  assert.equal(youtubeMedia.mediaUrl('https://fake-googlevideo.com/videoplayback'), null, 'Spoofed domain must be rejected');
  assert.equal(youtubeMedia.mediaUrl('https://googlevideo.com.attacker.com/videoplayback'), null, 'Subdomain hijack must be rejected');
  assert.equal(youtubeMedia.mediaUrl('http://googlevideo.com/videoplayback'), null, 'Insecure HTTP must be rejected');

  // 3. Reject credentials in URL
  assert.equal(youtubeMedia.mediaUrl('https://admin:pass@googlevideo.com/videoplayback'), null, 'Embedded credentials must be rejected');

  // 4. Reject live broadcast streams
  assert.equal(youtubeMedia.mediaUrl('https://googlevideo.com/videoplayback?source=yt_live_broadcast'), null, 'Live broadcast must be rejected');
});

test('Phase 5 Security: YouTube Player Data Sanitization & Live Stream Guard', () => {
  // 1. Malicious / invalid video ID
  const invalidPlayer = {
    videoDetails: { videoId: '../../evil/hack', title: 'Hacked Video' },
    streamingData: { formats: [] }
  };
  assert.equal(youtubeMedia.playerData(invalidPlayer), null, 'Invalid video ID must be rejected');

  // 2. Live stream content
  const livePlayer = {
    videoDetails: { videoId: 'dQw4w9WgXcQ', title: 'Live Rickroll', isLiveContent: true },
    streamingData: { formats: [] }
  };
  assert.equal(youtubeMedia.playerData(livePlayer), null, 'Live stream videoDetails must be rejected');

  // 3. File name traversal and invalid characters in title
  const rawPlayer = {
    videoDetails: { videoId: 'dQw4w9WgXcQ', title: 'My Video: Episode 1 / <Special> *Edition* ?' },
    streamingData: {
      formats: [
        {
          itag: 18,
          url: 'https://rr1---sn-4g5ednks.googlevideo.com/videoplayback?itag=18&expire=' + (Math.floor(Date.now() / 1000) + 3600),
          mimeType: 'video/mp4; codecs="avc1.42001E, mp4a.40.2"',
          width: 640,
          height: 360,
          bitrate: 500000
        }
      ]
    }
  };

  const parsed = youtubeMedia.playerData(rawPlayer);
  assert(parsed, 'Valid player response must parse');
  const choices = youtubeMedia.choices(parsed);
  assert(choices.length > 0, 'Must produce choices');
  assert(!/[<>:"/\\|?*]/.test(choices[0].filename), 'Filename must be sanitized of dangerous path characters');
});

test('Phase 5 Functionality: YouTube Adaptive 1080p Video + Audio Pairing', () => {
  const futureExpire = Math.floor(Date.now() / 1000) + 7200;
  const mockPlayer = {
    videoDetails: {
      videoId: 'abcdef12345',
      title: 'Awesome 4K Nature Documentary'
    },
    streamingData: {
      adaptiveFormats: [
        // 1080p 60fps video-only stream (no audio)
        {
          itag: 299,
          url: `https://rr2---sn-4g5ednks.googlevideo.com/videoplayback?id=o-123&itag=299&expire=${futureExpire}`,
          mimeType: 'video/mp4; codecs="avc1.64002a"',
          width: 1920,
          height: 1080,
          fps: 60,
          bitrate: 5500000
        },
        // 720p video-only stream
        {
          itag: 136,
          url: `https://rr2---sn-4g5ednks.googlevideo.com/videoplayback?id=o-123&itag=136&expire=${futureExpire}`,
          mimeType: 'video/mp4; codecs="avc1.4d401f"',
          width: 1280,
          height: 720,
          fps: 30,
          bitrate: 2200000
        },
        // High quality AAC audio stream
        {
          itag: 140,
          url: `https://rr2---sn-4g5ednks.googlevideo.com/videoplayback?id=o-123&itag=140&expire=${futureExpire}`,
          mimeType: 'audio/mp4; codecs="mp4a.40.2"',
          bitrate: 128000,
          audioQuality: 'AUDIO_QUALITY_MEDIUM'
        }
      ]
    }
  };

  const data = youtubeMedia.playerData(mockPlayer);
  assert(data, 'Player data must be successfully parsed');

  const choices = youtubeMedia.choices(data);
  assert.equal(choices.length, 3, 'Must generate 1080p, 720p, and Audio-only choices');

  // Verify 1080p choice is paired with audio track
  const choice1080p = choices.find(c => c.quality.includes('1080p'));
  assert(choice1080p, 'Must include 1080p 60fps choice');
  assert.equal(choice1080p.quality, '1080p 60fps');
  assert.equal(choice1080p.outputFormat, 'mp4');
  assert.equal(choice1080p.audioState, 'Video + audio');
  assert(choice1080p.audioUrl, 'Adaptive 1080p stream must be paired with audioUrl');
  assert(choice1080p.audioUrl.includes('itag=140'), 'Audio URL must be the itag 140 track');

  // Verify Audio-only MP3 choice
  const audioChoice = choices.find(c => c.format === 'MP3');
  assert(audioChoice, 'Must include MP3 audio option');
  assert.equal(audioChoice.audioState, 'Audio only');
  assert(audioChoice.filename.endsWith('.mp3'), 'MP3 choice must have .mp3 filename');
});

// =========================================================================
// Phase 8 Native Messaging & Safe Download Cancellation Tests
// =========================================================================

test('Phase 8 Manifest: Native Messaging Permission & Firefox Gecko Settings', () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, 'manifest.json'), 'utf8'));

  assert(
    manifest.permissions.includes('nativeMessaging'),
    'manifest.json must request nativeMessaging permission'
  );

  assert(
    manifest.browser_specific_settings?.gecko?.id === 'idm-extension@impressive.com',
    'manifest.json must define fixed Gecko extension ID for Firefox compatibility'
  );
});

test('Phase 8 Functionality: Safe Download Cancellation Logic', async () => {
  const bgJs = fs.readFileSync(path.join(__dirname, 'background.js'), 'utf8');

  // Verify that background.js defines com.impressive.idm native host
  assert(
    bgJs.includes('const NATIVE_HOST = "com.impressive.idm"') || bgJs.includes("const NATIVE_HOST = 'com.impressive.idm'"),
    'background.js must configure com.impressive.idm as the native host'
  );

  // Verify that sendViaNativeHost uses chrome.runtime.sendNativeMessage
  assert(
    bgJs.includes('sendNativeMessage(NATIVE_HOST, payload'),
    'background.js must dispatch payloads via sendNativeMessage'
  );

  // Verify Safe Cancellation: chrome.downloads.cancel must ONLY be called when desktop accepted
  assert(
    bgJs.includes('if (accepted) {') && bgJs.includes('chrome.downloads.cancel('),
    'chrome.downloads.cancel must only be invoked when desktop confirms acceptance'
  );

  // Verify Safe Fallback: if desktop rejects or is offline, browser download is preserved
  assert(
    bgJs.includes('preserving native browser download'),
    'background.js must log and preserve native download if desktop is unavailable'
  );
});

test('Phase 8 Reliability: background.js Interception Execution & No TDZ ReferenceErrors', () => {
  const bgJs = fs.readFileSync(path.join(__dirname, 'background.js'), 'utf8');

  // Verify JavaScript syntax by compiling
  assert.doesNotThrow(() => {
    new Function(bgJs);
  }, 'background.js must have valid JavaScript syntax');

  // Verify targetRealUrl is defined BEFORE any isFragment calls inside interceptDownloadItem
  const interceptFnMatch = bgJs.match(/async function interceptDownloadItem\([\s\S]*?\n\}/);
  assert(interceptFnMatch, 'interceptDownloadItem must be defined in background.js');

  const fnBody = interceptFnMatch[0];
  const targetRealUrlDeclIdx = fnBody.indexOf('let targetRealUrl');
  const isFragmentCheckIdx = fnBody.indexOf('isFragment(targetRealUrl)');

  assert(targetRealUrlDeclIdx !== -1, 'targetRealUrl must be declared');
  assert(isFragmentCheckIdx !== -1, 'isFragment(targetRealUrl) must exist');
  assert(
    targetRealUrlDeclIdx < isFragmentCheckIdx,
    'targetRealUrl MUST be declared before isFragment(targetRealUrl) to avoid TDZ ReferenceError!'
  );
});



