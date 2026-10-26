(function(root){
  function assetKey(value) {
    try {
      const u = new URL(value);
      if (!/(^|\.)twimg\.com$/.test(u.hostname)) return '';
      return /\/(?:amplify_video|ext_tw_video|tweet_video)(?:_thumb)?\/(\d+)/.exec(u.pathname)?.[1] || '';
    } catch { return ''; }
  }

  function mediaChoices(items, context = {}) {
    const wanted = assetKey(context.url) || assetKey(context.poster);
    if (items.some(i => i.provider === 'youtube')) return items.filter(i => i.provider === 'youtube');

    // Strict Fragment Filter: never show .m4s, .ts segments, or small fragment chunks
    let list = items.filter(i => !i.unsupported && !/\.(m4s|ts|aac|vtt|webvtt)(?:[?#]|$)/i.test(i.url) && !/^https:\/\/(?:www\.)?youtube\.com\/s\//.test(i.url));
    if (wanted) list = list.filter(i => assetKey(i.url) === wanted);

    const richAssets = new Set(list.filter(i => i.masterUrl).map(i => assetKey(i.url)).filter(Boolean));
    const audioUrls = new Set(list.map(i => i.audioUrl).filter(Boolean));
    list = list.filter(i => !audioUrls.has(i.url) && (!richAssets.has(assetKey(i.url)) || i.masterUrl));

    const seen = new Set(), choices = [];
    for (const item of list.sort((a, b) => (b.bitrate || 0) - (a.bitrate || 0))) {
      const hls = item.format === 'HLS';
      const audioState = item.audioUrl || /mp4a|ac-3|ec-3|opus/i.test(item.codecs || '')
        ? 'Video + audio'
        : item.codecs ? 'Video only' : 'Complete video';
      
      const video = {
        ...item,
        format: hls ? 'MP4' : (item.format || 'MP4'),
        outputFormat: hls ? 'mp4' : (item.outputFormat || 'mp4'),
        audioState,
        asset: assetKey(item.url)
      };

      const variants = [video];
      if (item.audioUrl) {
        variants.push({
          ...item,
          url: item.audioUrl,
          audioUrl: undefined,
          format: 'MP3',
          outputFormat: 'mp3',
          quality: 'Audio only',
          bitrate: 0,
          audioState: 'Audio only',
          asset: assetKey(item.url)
        });
      }

      for (const choice of variants) {
        const key = choice.url + '|' + (choice.outputFormat || '');
        if (!seen.has(key)) {
          seen.add(key);
          choices.push(choice);
        }
      }
    }
    return choices;
  }

  root.mediaChoices = mediaChoices;
  root.assetKey = assetKey;
  if (typeof module !== 'undefined') module.exports = { mediaChoices, assetKey };
})(globalThis);
