// Browser test for the GLB dragon: waits for the dragon.glb async load to
// finish (debug_dragon() reports the triangle count of the baked mesh),
// checks the flight path moves, and screenshots both view modes.
//
// Serve the web/ directory (python3 -m http.server 8090) then:
//   node tools/dragon-test.js

const puppeteer = require('puppeteer-core');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  const browser = await puppeteer.launch({
    headless: 'new',
    executablePath: '/usr/bin/google-chrome',
    args: ['--no-sandbox', '--window-size=1280,800'],
    defaultViewport: { width: 1280, height: 800 },
  });
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push('pageerror: ' + e.message));
  page.on('console', (m) => {
    // The boot-time config.ini probe 404s harmlessly when no config.ini
    // is checked in next to index.html — ignore bare 404 lines.
    if (m.type() === 'error' && !/404/.test(m.text())) errors.push('console: ' + m.text());
  });

  await page.goto('http://localhost:8090/', { waitUntil: 'networkidle0' });
  await page.evaluate(() => import('./pkg/gt6.js'));

  const dragon = () =>
    page.evaluate(async () => Array.from((await import('./pkg/gt6.js')).debug_dragon()));

  // --- Wait for the GLB to load (fetch + parse + bake, up to 25 s) ---
  let d = [0, 0, 0, 0, 0, 0, 0];
  const t0 = Date.now();
  while (Date.now() - t0 < 25000) {
    await sleep(500);
    d = await dragon();
    if (d[6] > 1000) break;
  }
  const loaded = d[6] > 1000;
  console.log(`${loaded ? 'PASS' : 'FAIL'}  dragon.glb loaded  tris=${d[6]}`);

  // --- The dragon flies: position changes between samples ---
  if (loaded) {
    await sleep(1500);
    const d2 = await dragon();
    const moved = Math.hypot(d2[0] - d[0], d2[1] - d[1]);
    console.log(
      `${moved > 50 ? 'PASS' : 'FAIL'}  dragon is flying  moved ${moved.toFixed(0)}px in 1.5s  z=${d2[2].toFixed(0)}`
    );
    if (d2[2] < 100 || d2[2] > 600) {
      console.log(`FAIL  dragon altitude in range  z=${d2[2].toFixed(0)}`);
    }
  }

  // --- Screenshot top-down (dragon shadow + silhouette over the city) ---
  await page.screenshot({ path: '/tmp/gt6_dragon_top.png' });

  // --- 3D view: teleport beside the dragon and aim the camera at it so
  // the full GLB mesh (not the far silhouette) is in frame ---
  const m = await page.evaluate(async () => {
    const mm = await import('./pkg/gt6.js');
    const d = Array.from(mm.debug_dragon());
    mm.debug_teleport(d[0], d[1], 0);
    return d;
  });
  await sleep(400);
  await page.evaluate(async () => (await import('./pkg/gt6.js')).debug_dragon_focus());
  await sleep(1500);
  await page.screenshot({ path: '/tmp/gt6_dragon_3d.png' });
  console.log(`INFO  dragon at (${m[0].toFixed(0)}, ${m[1].toFixed(0)}) z=${m[2].toFixed(0)}`);

  if (errors.length) {
    console.log('FAIL  no page/console errors  ' + errors.join(' | '));
  } else {
    console.log('PASS  no page/console errors');
  }

  await browser.close();
  process.exit(loaded && errors.length === 0 ? 0 : 1);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
