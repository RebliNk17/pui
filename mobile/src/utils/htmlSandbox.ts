// Sandboxed CSP wrapper for `pui_visual` inline HTML/CSS/SVG cards and HTML artifacts.
// Mirrors Pui Desktop (`src/inline_visual.rs`): inline styles/scripts/SVG + data: images only;
// blocks external network requests, forms, frames, and local-file access.

export const SANDBOX_CSP =
  "default-src 'none'; img-src data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; font-src data:; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none';";

export function wrapSandboxedHtml(rawHtml: string, darkMode: boolean): string {
  const trimmed = rawHtml.trim();
  const cspMeta = `<meta http-equiv="Content-Security-Policy" content="${SANDBOX_CSP}">`;
  const viewportMeta =
    '<meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=2.0">';
  const autoResizeScript = `<script>
(function(){
  function postHeight(){
    try {
      var h = Math.max(document.documentElement.scrollHeight || 0, document.body ? document.body.scrollHeight : 0);
      if (window.ReactNativeWebView && h > 0) {
        window.ReactNativeWebView.postMessage(JSON.stringify({ type: 'height', height: Math.min(Math.max(h, 120), 1400) }));
      }
    } catch (_) {}
  }
  window.addEventListener('load', postHeight);
  setTimeout(postHeight, 120);
  setTimeout(postHeight, 450);
})();
</script>`;

  const hasHtmlShell = /<html[\s>]/i.test(trimmed) || /<!doctype\s+html/i.test(trimmed);
  if (hasHtmlShell) {
    if (/<head[\s>]/i.test(trimmed)) {
      const withMeta = trimmed.replace(/<head([^>]*)>/i, `<head$1>${cspMeta}${viewportMeta}`);
      return /<\/body>/i.test(withMeta)
        ? withMeta.replace(/<\/body>/i, `${autoResizeScript}</body>`)
        : `${withMeta}${autoResizeScript}`;
    }
    return `<!doctype html><html><head>${cspMeta}${viewportMeta}</head><body>${trimmed}${autoResizeScript}</body></html>`;
  }

  const bg = darkMode ? '#14171f' : '#ffffff';
  const fg = darkMode ? '#e6edf3' : '#1f2328';
  return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
${cspMeta}
${viewportMeta}
<style>
  html, body { margin: 0; padding: 10px; background: ${bg}; color: ${fg}; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
  svg { max-width: 100%; height: auto; }
</style>
</head>
<body>${trimmed}${autoResizeScript}</body>
</html>`;
}
