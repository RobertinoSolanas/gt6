const puppeteer = require('puppeteer-core');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  const browser = await puppeteer.launch({
    headless: 'new',
    executablePath: '/usr/bin/google-chrome',
    args: ['--no-sandbox', '--autoplay-policy=no-user-gesture-required', '--window-size=1280,800'],
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
  await sleep(1500);

  const pos = () =>
    page.evaluate(async () => {
      const m = await import('./pkg/gt6.js');
      const info = Array.from(m.debug_player_info());
      return { speed: m.debug_player_speed(), pos: info, onFoot: info[2] };
    });

  const key = (k, down) =>
    page.evaluate(async ({ k, down }) => {
      window.dispatchEvent(
        new KeyboardEvent(down ? 'keydown' : 'keyup', { key: k })
      );
    }, { k, down });

  const results = {};

  // --- Test 1: F1 makes the parked car drive itself ---
  let a = await pos();
  await key('f1', true);
  await key('f1', false);
  await sleep(2500);
  let b = await pos();
  const moved = Math.hypot((b.pos && b.pos[0] - a.pos[0]) || 0, (b.pos && b.pos[1] - a.pos[1]) || 0);
  results.f1_car_speed = b.speed;
  results.f1_car_moved = moved;
  results.f1_car_ok = b.speed > 50;
  await key('f1', true);
  await key('f1', false); // turn auto off

  // --- Test 2: RMB walks the player forward on foot ---
  // Get out of the car (must be stopped) then hold RMB.
  await key('s', true);
  await sleep(1200);
  await key('s', false);
  await key('e', true);
  await key('e', false);
  await sleep(200);
  let c = await pos();
  await page.mouse.down({ button: 'right' });
  await sleep(1000);
  await page.mouse.up({ button: 'right' });
  let d = await pos();
  const walked = Math.hypot((d.pos && d.pos[0] - c.pos[0]) || 0, (d.pos && d.pos[1] - c.pos[1]) || 0);
  results.rmb_walk = walked;
  results.rmb_ok = walked > 10;

  console.log(JSON.stringify(results, null, 2));
  console.log('ERRORS:', errors.length ? errors : 'none');

  const ok = results.f1_car_ok && results.rmb_ok && errors.length === 0;
  console.log(ok ? 'AUTO-MODE TEST: PASS' : 'AUTO-MODE TEST: FAIL');
  await browser.close();
  process.exit(ok ? 0 : 1);
})().catch((e) => {
  console.error('THREW', e);
  process.exit(2);
});
