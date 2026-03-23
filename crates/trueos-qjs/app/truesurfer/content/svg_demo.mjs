export const SVG_DEMO_ROUTE = 'trueos://ui/svg-demo';

export const SVG_FIXTURES = [
  {
    name: 'sunrise_layers',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#132a4f"/>
      <stop offset="55%" stop-color="#f26b5b"/>
      <stop offset="100%" stop-color="#ffd27a"/>
    </linearGradient>
    <radialGradient id="sun" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff3bf"/>
      <stop offset="100%" stop-color="#ff9f43"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" fill="url(#sky)"/>
  <circle cx="48" cy="38" r="18" fill="url(#sun)"/>
  <path d="M0 64 C10 58 20 56 32 60 C42 63 54 66 66 62 C78 58 87 59 96 64 L96 96 L0 96 Z" fill="#553c66"/>
  <path d="M0 74 C10 70 20 67 32 70 C42 73 56 76 70 72 C82 68 90 69 96 72 L96 96 L0 96 Z" fill="#2c2348"/>
  <path d="M0 84 C12 80 23 78 34 81 C46 84 58 87 70 84 C81 81 90 82 96 84 L96 96 L0 96 Z" fill="#161126"/>
</svg>`,
  },
  {
    name: 'ribbon_flower',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="petal" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#ff8fb1"/>
      <stop offset="100%" stop-color="#ff4d6d"/>
    </linearGradient>
    <radialGradient id="core" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff4b5"/>
      <stop offset="100%" stop-color="#ffb703"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" fill="#fff7ef"/>
  <g fill="url(#petal)" stroke="#7a284a" stroke-width="2" stroke-linejoin="round">
    <path d="M48 18 C60 22 66 31 66 42 C58 45 52 45 48 42 C44 45 38 45 30 42 C30 31 36 22 48 18 Z"/>
    <path d="M78 48 C74 60 65 66 54 66 C51 58 51 52 54 48 C51 44 51 38 54 30 C65 30 74 36 78 48 Z"/>
    <path d="M48 78 C36 74 30 65 30 54 C38 51 44 51 48 54 C52 51 58 51 66 54 C66 65 60 74 48 78 Z"/>
    <path d="M18 48 C22 36 31 30 42 30 C45 38 45 44 42 48 C45 52 45 58 42 66 C31 66 22 60 18 48 Z"/>
  </g>
  <circle cx="48" cy="48" r="10" fill="url(#core)" stroke="#8c5a00" stroke-width="2"/>
</svg>`,
  },
  {
    name: 'radar_ping',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <radialGradient id="glow" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#8ff7c8" stop-opacity="0.95"/>
      <stop offset="100%" stop-color="#0d3b2a" stop-opacity="0.15"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" rx="12" fill="#091a16"/>
  <circle cx="48" cy="48" r="28" fill="url(#glow)"/>
  <circle cx="48" cy="48" r="12" fill="none" stroke="#7df9c1" stroke-width="2"/>
  <circle cx="48" cy="48" r="24" fill="none" stroke="#4dd9a6" stroke-width="2" stroke-opacity="0.8"/>
  <circle cx="48" cy="48" r="36" fill="none" stroke="#2ca67f" stroke-width="2" stroke-opacity="0.6"/>
  <path d="M48 48 L76 34 A32 32 0 0 1 80 48 Z" fill="#8ff7c8" fill-opacity="0.35"/>
  <path d="M48 14 L48 82 M14 48 L82 48" stroke="#74e7b7" stroke-width="1.5" stroke-linecap="round"/>
  <circle cx="48" cy="48" r="4" fill="#d7fff0"/>
</svg>`,
  },
  {
    name: 'candy_badge',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="shell" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#8ec5ff"/>
      <stop offset="100%" stop-color="#2d7ff9"/>
    </linearGradient>
    <linearGradient id="spark" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.95"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" fill="#f3f8ff"/>
  <path d="M48 14 L76 28 L76 62 C76 74 64 82 48 86 C32 82 20 74 20 62 L20 28 Z" fill="url(#shell)" stroke="#14439a" stroke-width="3" stroke-linejoin="round"/>
  <path d="M48 26 L66 35 L66 58 C66 66 58 72 48 75 C38 72 30 66 30 58 L30 35 Z" fill="#e9f3ff" fill-opacity="0.35"/>
  <path d="M34 28 C42 24 50 24 58 28 C50 31 42 37 36 48 C33 42 32 35 34 28 Z" fill="url(#spark)"/>
  <path d="M34 54 C38 49 43 46 48 46 C53 46 58 49 62 54 C58 60 53 64 48 66 C43 64 38 60 34 54 Z M43 54 C45 52 46 51 48 51 C50 51 51 52 53 54 C51 56 50 57 48 59 C46 57 45 56 43 54 Z" fill="#ffffff" fill-rule="evenodd"/>
</svg>`,
  },
  {
    name: 'wave_tiles',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#132238"/>
      <stop offset="100%" stop-color="#214d6b"/>
    </linearGradient>
    <linearGradient id="waveA" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#6ee7f9"/>
      <stop offset="100%" stop-color="#3b82f6"/>
    </linearGradient>
    <linearGradient id="waveB" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#f9a8d4"/>
      <stop offset="100%" stop-color="#f97316"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="14" fill="url(#bg)"/>
  <path d="M8 28 C20 16 34 16 46 28 C58 40 72 40 88 28" fill="none" stroke="url(#waveA)" stroke-width="8" stroke-linecap="round"/>
  <path d="M8 48 C20 36 34 36 46 48 C58 60 72 60 88 48" fill="none" stroke="url(#waveB)" stroke-width="8" stroke-linecap="round"/>
  <path d="M8 68 C20 56 34 56 46 68 C58 80 72 80 88 68" fill="none" stroke="url(#waveA)" stroke-width="8" stroke-linecap="round"/>
  <circle cx="20" cy="78" r="4" fill="#f8fafc"/>
  <circle cx="48" cy="18" r="3" fill="#f8fafc" fill-opacity="0.8"/>
  <circle cx="76" cy="78" r="4" fill="#f8fafc"/>
</svg>`,
  },
  {
    name: 'comet_loop',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <radialGradient id="head" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff6d6"/>
      <stop offset="100%" stop-color="#ffb347"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" fill="#090b1a"/>
  <path d="M20 72 C16 54 20 34 34 24 C46 16 62 16 72 24 C82 32 82 48 72 56 C62 64 46 64 34 56 C24 49 24 38 32 32 C39 27 49 27 56 32" fill="none" stroke="#7dd3fc" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M18 76 C30 68 42 64 54 64 C44 70 32 78 24 88 Z" fill="#7dd3fc" fill-opacity="0.35"/>
  <circle cx="58" cy="34" r="8" fill="url(#head)" stroke="#ffedd5" stroke-width="1.5"/>
  <circle cx="70" cy="22" r="2" fill="#ffffff"/>
  <circle cx="78" cy="30" r="1.5" fill="#ffffff" fill-opacity="0.8"/>
</svg>`,
  },
  {
    name: 'weather_sun',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <radialGradient id="sunCore" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff7cc"/>
      <stop offset="100%" stop-color="#ffb703"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#e6f6ff"/>
  <circle cx="48" cy="48" r="18" fill="url(#sunCore)" stroke="#d97706" stroke-width="2.5"/>
  <path d="M48 10 L48 22 M48 74 L48 86 M10 48 L22 48 M74 48 L86 48 M21 21 L29 29 M67 67 L75 75 M21 75 L29 67 M67 29 L75 21" stroke="#f59e0b" stroke-width="4" stroke-linecap="round"/>
</svg>`,
  },
  {
    name: 'weather_partly_cloudy',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <radialGradient id="smallSun" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff4bf"/>
      <stop offset="100%" stop-color="#f59e0b"/>
    </radialGradient>
    <linearGradient id="cloud" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#f8fbff"/>
      <stop offset="100%" stop-color="#cbdcf2"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#dff3ff"/>
  <circle cx="34" cy="32" r="14" fill="url(#smallSun)" stroke="#d97706" stroke-width="2"/>
  <path d="M34 10 L34 16 M34 48 L34 54 M12 32 L18 32 M50 32 L56 32 M18 18 L22 22 M46 42 L50 46 M18 46 L22 42 M46 22 L50 18" stroke="#f59e0b" stroke-width="3" stroke-linecap="round"/>
  <path d="M28 62 C28 53 35 46 44 46 C47 46 50 47 53 49 C56 42 63 38 71 38 C82 38 90 47 90 58 C90 69 82 78 71 78 L44 78 C35 78 28 71 28 62 Z" fill="url(#cloud)" stroke="#7b93b7" stroke-width="2.5" stroke-linejoin="round"/>
</svg>`,
  },
  {
    name: 'weather_cloudy',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="cloudBack" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#e7eef8"/>
      <stop offset="100%" stop-color="#b8c6d9"/>
    </linearGradient>
    <linearGradient id="cloudFront" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff"/>
      <stop offset="100%" stop-color="#d9e5f3"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#eaf2f8"/>
  <path d="M18 56 C18 48 24 42 32 42 C35 42 38 43 40 45 C43 39 49 35 56 35 C66 35 74 43 74 53 C74 63 66 71 56 71 L32 71 C24 71 18 64 18 56 Z" fill="url(#cloudBack)" stroke="#8a9aad" stroke-width="2"/>
  <path d="M28 62 C28 53 35 46 44 46 C47 46 50 47 53 49 C56 42 63 38 71 38 C82 38 90 47 90 58 C90 69 82 78 71 78 L44 78 C35 78 28 71 28 62 Z" fill="url(#cloudFront)" stroke="#7b93b7" stroke-width="2.5"/>
</svg>`,
  },
  {
    name: 'weather_rain',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="rainCloud" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#f7fbff"/>
      <stop offset="100%" stop-color="#d2ddea"/>
    </linearGradient>
    <linearGradient id="drop" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#7dd3fc"/>
      <stop offset="100%" stop-color="#2563eb"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#edf6ff"/>
  <path d="M22 52 C22 43 29 36 38 36 C41 36 45 37 48 39 C51 32 58 28 66 28 C77 28 86 37 86 48 C86 60 77 69 66 69 L38 69 C29 69 22 61 22 52 Z" fill="url(#rainCloud)" stroke="#7b93b7" stroke-width="2.5"/>
  <path d="M34 74 C36 68 39 64 42 60 C45 64 48 68 50 74 C50 78 46 82 42 82 C38 82 34 78 34 74 Z" fill="url(#drop)"/>
  <path d="M50 80 C52 74 55 70 58 66 C61 70 64 74 66 80 C66 84 62 88 58 88 C54 88 50 84 50 80 Z" fill="url(#drop)"/>
  <path d="M66 74 C68 68 71 64 74 60 C77 64 80 68 82 74 C82 78 78 82 74 82 C70 82 66 78 66 74 Z" fill="url(#drop)"/>
</svg>`,
  },
  {
    name: 'weather_thunder',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="stormCloud" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#dde5f1"/>
      <stop offset="100%" stop-color="#97a7bd"/>
    </linearGradient>
    <linearGradient id="bolt" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#fff3a3"/>
      <stop offset="100%" stop-color="#facc15"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#e8edf5"/>
  <path d="M20 50 C20 41 27 34 36 34 C40 34 43 35 46 37 C49 30 56 26 64 26 C76 26 86 36 86 48 C86 60 76 70 64 70 L36 70 C27 70 20 62 20 50 Z" fill="url(#stormCloud)" stroke="#6f8197" stroke-width="2.5"/>
  <path d="M52 48 L42 66 L50 66 L44 86 L66 60 L56 60 L64 48 Z" fill="url(#bolt)" stroke="#ca8a04" stroke-width="2" stroke-linejoin="round"/>
</svg>`,
  },
  {
    name: 'weather_snow',
    svg: `
<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="snowCloud" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#f9fcff"/>
      <stop offset="100%" stop-color="#d7e2ef"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#eef7ff"/>
  <path d="M22 50 C22 41 29 34 38 34 C41 34 45 35 48 37 C51 30 58 26 66 26 C77 26 86 35 86 46 C86 58 77 67 66 67 L38 67 C29 67 22 59 22 50 Z" fill="url(#snowCloud)" stroke="#88a0bb" stroke-width="2.5"/>
  <path d="M34 76 L42 76 M38 72 L38 80 M35 73 L41 79 M41 73 L35 79" stroke="#67b7ff" stroke-width="2.5" stroke-linecap="round"/>
  <path d="M54 84 L62 84 M58 80 L58 88 M55 81 L61 87 M61 81 L55 87" stroke="#67b7ff" stroke-width="2.5" stroke-linecap="round"/>
  <path d="M70 76 L78 76 M74 72 L74 80 M71 73 L77 79 M77 73 L71 79" stroke="#67b7ff" stroke-width="2.5" stroke-linecap="round"/>
</svg>`,
  },
];

function escapeHtml(value) {
  return String(value || '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

export function svgFixtureDataUrl(svg) {
  return `data:image/svg+xml;utf8,${encodeURIComponent(String(svg || ''))}`;
}

export function createSvgDemoHtml(options = {}) {
  const title = typeof options.title === 'string' && options.title.trim()
    ? options.title.trim()
    : 'SVG Demo';
  const iconSize = Math.max(1, Math.round(Number(options.iconSize || 64) || 64));
  const rows = SVG_FIXTURES.map((fixture) => {
    const name = escapeHtml(fixture.name);
    const src = escapeHtml(svgFixtureDataUrl(fixture.svg));
    return `<tr><td><img src="${src}" alt="${name}" width="${iconSize}" height="${iconSize}"></td><td><code>${name}</code></td></tr>`;
  }).join('\n');

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>${escapeHtml(title)}</title>
</head>
<body>
  <h1>${escapeHtml(title)}</h1>
  <p>Simple ${iconSize}x${iconSize} SVG list for the browser app.</p>
  <table border="1">
    <tr>
      <th>Icon</th>
      <th>Name</th>
    </tr>
    ${rows}
  </table>
</body>
</html>`;
}

export function registerSvgDemoRoute(host = globalThis, options = {}) {
  const runtimeHost = host && typeof host === 'object' ? host : globalThis;
  if (!runtimeHost.__trueosBrowserEmbeddedRoutes || typeof runtimeHost.__trueosBrowserEmbeddedRoutes !== 'object') {
    runtimeHost.__trueosBrowserEmbeddedRoutes = Object.create(null);
  }
  const html = createSvgDemoHtml(options);
  runtimeHost.__trueosBrowserEmbeddedRoutes[SVG_DEMO_ROUTE] = html;
  runtimeHost.__trueosBrowserSvgDemoHtml = html;
  runtimeHost.__trueosBrowserSvgFixtures = SVG_FIXTURES.slice();
  return {
    url: SVG_DEMO_ROUTE,
    html,
    count: SVG_FIXTURES.length,
  };
}
