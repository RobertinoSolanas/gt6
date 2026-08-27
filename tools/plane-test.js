// Headless flight test: summon the airplane with F, fly it with the mouse
// (drag to steer, LMB throttle, RMB brake, wheel for cruise), and land.
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
  const gt = (fn, ...args) =>
    page.evaluate(([f, a]) => import('./pkg/gt6.js').then((m) => m[f](...a)), [fn, args]);
  await sleep(1500);

  // --- F: summon the plane straight from the spawn car ---
  await page.keyboard.press('f');
  await sleep(300);
  console.log(`alt after F (should ease off the street): ${(await gt('debug_player_alt')).toFixed(1)}`);

  // --- Mouse climb: drag up while LMB (full throttle) is held ---
  // Pitch is rate-based: a fast flick of the mouse = full pitch.
  await page.mouse.move(640, 400);
  await page.mouse.down(); // LMB = full throttle
  for (let i = 0; i < 12; i++) {
    await page.mouse.move(640 + i * 3, 400 - i * 15, { steps: 1 });
    await sleep(100);
  }
  const alt1 = await gt('debug_player_alt');
  const spd = await gt('debug_player_speed');
  console.log(`alt after mouse climb: ${alt1.toFixed(1)}  speed: ${spd.toFixed(0)} px/s`);

  // --- Mouse yaw: drag right ---
  const pos = await page.evaluate(() => {
    return import('./pkg/gt6.js').then((m) => m.debug_player_info());
  }).then(Array.from);
  for (let i = 0; i < 8; i++) {
    await page.mouse.move(640 + 150 + i * 8, 300, { steps: 1 }); // drag right = yaw right
    await sleep(100);
  }
  const pos2 = await page.evaluate(() => {
    return import('./pkg/gt6.js').then((m) => m.debug_player_info());
  }).then(Array.from);
  const moved = Math.hypot(pos2[0] - pos[0], pos2[1] - pos[1]);
  console.log(`mouse yaw moved player ${moved.toFixed(0)}px over 0.8s`);

  // --- Wheel: drop the cruise throttle ---
  const thr0 = await gt('debug_mouse_throttle');
  await page.mouse.up(); // release LMB -> falls back to cruise throttle
  await page.mouse.wheel({ deltaY: 3 * 120 }); // three notches down
  await sleep(200);
  const thr1 = await gt('debug_mouse_throttle');
  console.log(`cruise throttle ${thr0.toFixed(2)} -> ${thr1.toFixed(2)} after wheel down`);

  // --- 3D chase cam high above the city — screenshot ---
  await page.keyboard.down('w');
  await page.keyboard.press('v');
  await sleep(1200);
  await page.screenshot({ path: '/tmp/gt6_plane_fly.png' });
  await page.keyboard.up('w');

  // --- M: auto-land at the nearest safe space ---
  await page.keyboard.up('w');
  await page.keyboard.press('m');
  let autoLand = await gt('debug_landing');
  console.log(`auto-land active after M: ${autoLand}`);
  const alt3 = await gt('debug_player_alt');
  for (let i = 0; i < 200; i++) {
    await sleep(250);
    if ((await gt('debug_landing')) === 0) break;
  }
  const alt2 = await gt('debug_player_alt');
  const spd2 = await gt('debug_player_speed');
  console.log(`auto-land: alt ${alt3.toFixed(0)} -> ${alt2.toFixed(1)}, final speed ${spd2.toFixed(0)} px/s`);
  await page.keyboard.press('e');
  await sleep(300);
  console.log(`alt after exit: ${(await gt('debug_player_alt')).toFixed(1)}`);

  let pass = true;
  const check = (name, ok) => {
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}`);
    if (!ok) pass = false;
  };
  check('F summoned the plane and eased it up', (await gt('debug_player_alt')) >= 0);
  check('mouse climb got above rooftops', alt1 > 250);
  check('LMB reached airspeed', spd > 400);
  check('mouse drag yawed the flight path', moved > 50);
  check('wheel lowered the cruise throttle', thr1 < thr0);
  check('M started the auto-land', autoLand === 1);
  check('auto-land set the plane down', alt2 < 1 && spd2 < 10);
  if (errors.length) {
    pass = false;
    console.log('PAGE ERRORS:');
    for (const e of errors) console.log('  ' + e);
  }
  console.log(pass ? 'ALL FLIGHT TESTS PASSED' : 'FLIGHT TEST FAILED');
  await browser.close();
  process.exit(pass ? 0 : 1);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
