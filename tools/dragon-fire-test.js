// Browser test for the dragon's breath of fire: mount the dragon (D), dive
// low, stream fireballs with the left mouse button, watch buildings crash
// down and burn, then drop to the street and watch the citizens rush in
// with water. Screenshots: /tmp/gt6_dragon_fire.png (3D fireball),
// /tmp/gt6_fire_city.png (burning city + crowd, top-down).
//
// Serve the web/ directory (python3 -m http.server 8090) then:
//   node tools/dragon-fire-test.js

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
    if (m.type() === 'error') errors.push('console: ' + m.text());
  });

  await page.goto('http://localhost:8090/', { waitUntil: 'networkidle0' });
  await page.evaluate(() => import('./pkg/gt6.js'));
  await sleep(1500);

  const dragon = () => page.evaluate(async () => Array.from((await import('./pkg/gt6.js')).debug_dragon()));
  const fire = () => page.evaluate(async () => Array.from((await import('./pkg/gt6.js')).debug_dragonfire()));

  let ok = true;
  const log = (name, pass, detail) => {
    ok = ok && pass;
    console.log(`${pass ? 'PASS' : 'FAIL'}  ${name}  ${detail}`);
  };

  // Wait for the GLB dragon mesh to finish loading.
  for (let i = 0; i < 40; i++) {
    if ((await dragon())[6] > 1000) break;
    await sleep(250);
  }
  log('dragon mesh loaded', (await dragon())[6] > 1000, `tris=${(await dragon())[6]}`);

  // Mount the dragon (G is the dedicated summon key).
  await page.keyboard.down('g');
  await sleep(300);
  await page.keyboard.up('g');
  await sleep(300);
  log('G mounts the dragon', (await dragon())[7] === 1, `in_dragon=${(await dragon())[7]}`);

  // Dive low over the rooftops so the fireballs hit the city.
  await page.keyboard.down(' ');
  await sleep(2500);
  await page.keyboard.up(' ');
  await sleep(300);
  const dLow = await dragon();
  log('dragon dove low over the city', dLow[2] < 200, `alt=${dLow[2].toFixed(0)}`);

  // Stream fireballs with the left mouse button.
  await page.mouse.move(640, 400);
  await page.mouse.down();
  let inFlight = 0;
  let burned = 0;
  for (let i = 0; i < 14; i++) {
    await sleep(300);
    const f = await fire();
    inFlight = Math.max(inFlight, f[0]);
    burned = Math.max(burned, f[1]);
  }
  await page.screenshot({ path: '/tmp/gt6_dragon_fire.png' });
  await page.mouse.up();
  log('left mouse fires fireballs', inFlight > 0, `fireballs seen in flight=${inFlight}`);
  log('buildings burning', burned > 0, `burning buildings=${burned}`);

  // Give the collapses a moment to finish, then drop to the street to watch
  // the citizens fight the fire (the dragon stays where it was).
  await sleep(2500);
  await page.keyboard.down('d');
  await sleep(300);
  await page.keyboard.up('d');
  await sleep(300);
  const f2 = await fire();
  log('still burning on the ground', f2[1] > 0, `burning buildings=${f2[1]}`);

  // Wait for the street crowd to enlist and throw water.
  let crew = 0;
  for (let i = 0; i < 30; i++) {
    await sleep(500);
    crew = (await fire())[2];
    if (crew > 0) break;
  }
  log('citizens rushed to fight the fire', crew > 0, `firefighters on duty=${crew}`);

  // Top-down view of the burning block + the water crew.
  await page.keyboard.down('v');
  await sleep(200);
  await page.keyboard.up('v');
  await sleep(1200);
  await page.screenshot({ path: '/tmp/gt6_fire_city.png' });

  if (errors.length) {
    ok = false;
    console.log('FAIL  no page errors  ' + errors.join(' | '));
  } else {
    console.log('PASS  no page errors');
  }

  await browser.close();
  process.exit(ok ? 0 : 1);
})();
