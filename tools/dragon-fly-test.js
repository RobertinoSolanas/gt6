// Browser test for the dragon-control mode ("G"): mounts the dragon, flies it
// with the keyboard (throttle + climb) and the mouse, screenshots the 3D chase
// cam, then releases it back to the ground.
//
// Serve the web/ directory (python3 -m http.server 8090) then:
//   node tools/dragon-fly-test.js

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
  await sleep(1500); // let the game boot & settle

  // debug_dragon() -> [x, y, z, heading, flap, bank, tris, in_dragon, speed]
  const dragon = () =>
    page.evaluate(async () => Array.from((await import('./pkg/gt6.js')).debug_dragon()));
  const info = () =>
    page.evaluate(async () => {
      const m = await import('./pkg/gt6.js');
      return { alt: m.debug_player_alt(), speed: m.debug_player_speed() };
    });

  let ok = true;
  const log = (name, pass, detail) => {
    ok = ok && pass;
    console.log(`${pass ? 'PASS' : 'FAIL'}  ${name}  ${detail}`);
  };

  // --- Mount the dragon (G is the dedicated summon key) ---
  await page.keyboard.down('g');
  await sleep(300);
  await page.keyboard.up('g');
  await sleep(200);
  let d = await dragon();
  let mode = await info();
  log('G mounts the dragon', d[7] === 1, `in_dragon=${d[7]}  view=3D  alt=${mode.alt.toFixed(0)}`);

  // --- Throttle + climb: the dragon should gain speed and altitude ---
  await page.keyboard.down('w');
  await page.keyboard.down('Shift');
  await sleep(1800);
  const dClimb = await dragon();
  const climbRise = dClimb[2] - d[2];
  log(
    'W+Shift climbs and accelerates',
    dClimb[8] > 100 && climbRise > 20,
    `speed=${dClimb[8].toFixed(0)}  rose ${climbRise.toFixed(0)}m`
  );
  await page.keyboard.up('w');
  await page.keyboard.up('Shift');

  // --- Steer with the mouse (drag left/right turns the dragon) ---
  // A wide, slow drag accumulates enough steer to bank the dragon around.
  const before = await dragon();
  await page.mouse.move(300, 400);
  await page.mouse.down();
  await page.mouse.move(1050, 400, { steps: 60 });
  await page.mouse.up();
  await sleep(500);
  const after = await dragon();
  const turn = Math.abs(((after[3] - before[3]) % (Math.PI * 2)));
  log('mouse drag steers the dragon', turn > 0.15, `turn ${((turn * 180) / Math.PI).toFixed(0)}deg`);

  // --- Screenshot the 3D chase cam (dragon flying ahead of the camera) ---
  await sleep(500);
  await page.screenshot({ path: '/tmp/gt6_dragon_fly.png' });

  // --- Release: drop back to the street below, on foot ---
  await page.keyboard.down('e');
  await sleep(300);
  await page.keyboard.up('e');
  await sleep(300);
  const dRel = await dragon();
  const relInfo = await info();
  log(
    'E releases the dragon',
    dRel[7] === 0 && relInfo.alt === 0,
    `in_dragon=${dRel[7]}  alt=${relInfo.alt.toFixed(0)}`
  );
  await page.screenshot({ path: '/tmp/gt6_dragon_release.png' });

  if (errors.length) {
    log('no page/console errors', false, errors.join(' | '));
  } else {
    log('no page/console errors', true, 'clean');
  }

  await browser.close();
  process.exit(ok ? 0 : 1);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
