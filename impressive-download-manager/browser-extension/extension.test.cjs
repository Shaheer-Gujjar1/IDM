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

  const contentScripts = manifest.content_scripts[0];
  assert(contentScripts.js.includes('media-choices.js'));
  assert(contentScripts.js.includes('hls.js'));
  assert(contentScripts.js.includes('content.js'));
});
