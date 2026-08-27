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
  await sleep(1500); // let the game boot & run

  const info = () =>
    page.evaluate(async () => {
      const m = await import('./pkg/gt6.js');
      return {
        speed: m.debug_player_speed(),
        info: Array.from(m.debug_player_info()),
      };
    });

  let results = [];
  const log = (name, pass, detail) => {
    results.push(`${pass ? 'PASS' : 'FAIL'}  ${name}  ${detail}`);
  };

  // --- Test 1: idle, speed ~0 ---
  let s = (await info()).speed;
  log('idle speed ~0', s < 5, `speed=${s.toFixed(1)}`);

  // --- Test 2: hold W -> speed rises ---
  await page.keyboard.down('w');
  await sleep(1500);
  s = (await info()).speed;
  log('W accelerates', s > 100, `speed=${s.toFixed(1)} px/s`);

  // --- Test 3: position actually changes ---
  let p1 = (await info()).info;
  await sleep(700);
  let p2 = (await info()).info;
  const moved = Math.hypot(p2[0] - p1[0], p2[1] - p1[1]);
  log('position advances', moved > 200, `moved ${moved.toFixed(0)}px in 0.7s`);

  // --- Test 4: steering changes heading (position curve) ---
  await page.keyboard.up('w');
  await page.keyboard.down('w');
  await page.keyboard.down('d');
  await sleep(800);
  await page.keyboard.up('d');
  await page.keyboard.up('w');
  await sleep(1200); // coast to near-stop

  // --- Test 5: S reverses (moves backward) ---
  let b1 = (await info()).info;
  await page.keyboard.down('s');
  await sleep(1200);
  await page.keyboard.up('s');
  let b2 = (await info()).info;
  const revMoved = Math.hypot(b2[0] - b1[0], b2[1] - b1[1]);
  log('S brakes/reverses', revMoved > 100, `moved ${revMoved.toFixed(0)}px`);

  // --- Test 6: E exits to on-foot (car must be nearly stopped first) ---
  await page.keyboard.up('s');
  let spd;
  do {
    await sleep(400);
    spd = (await info()).speed;
  } while (spd > 40 && spd !== 0);
  await page.keyboard.press('e');
  await sleep(400);
  let f = (await info()).info;
  log('E exits car (on_foot)', f[2] === true, `on_foot=${f[2]}`);

  // --- Test 7: on-foot W moves the pedestrian ---
  let fp1 = (await info()).info;
  await page.keyboard.down('w');
  await sleep(800);
  await page.keyboard.up('w');
  let fp2 = (await info()).info;
  const footMoved = Math.hypot(fp2[0] - fp1[0], fp2[1] - fp1[1]);
  log('on-foot walking', footMoved > 50, `moved ${footMoved.toFixed(0)}px in 0.8s`);

  // --- Test 8: E re-enters car (walk back toward it first) ---
  await page.keyboard.down('s'); // walk back south toward the car
  await sleep(900);
  await page.keyboard.up('s');
  await page.keyboard.press('e');
  await sleep(400);
  f = (await info()).info;
  if (f[2]) {
    // maybe still a bit far; try again after one more step back
    await page.keyboard.down('s');
    await sleep(400);
    await page.keyboard.up('s');
    await page.keyboard.press('e');
    await sleep(400);
    f = (await info()).info;
  }
  log('E re-enters car', f[2] === false, `on_foot=${f[2]}`);

  // --- Test 9: arrow keys also drive ---
  // The driving above earns wanted heat; with a working police pursuit a
  // BUSTED screen would freeze the car, so clear it before precision tests.
  await page.evaluate(async () => (await import('./pkg/gt6.js')).debug_clear_heat());
  await sleep(200);
  await page.keyboard.down('ArrowUp');
  await sleep(1200);
  s = (await info()).speed;
  await page.keyboard.up('ArrowUp');
  log('ArrowUp accelerates', s > 100, `speed=${s.toFixed(1)}`);

  // --- Test 10: handbrake (no throttle) brakes hard ---
  // make sure we are actually in the car for this one
  if ((await info()).info[2]) {
    await page.keyboard.press('e');
    await sleep(300);
  }
  await page.keyboard.down('w');
  await sleep(1000); // build speed
  await page.keyboard.up('w');
  await sleep(200);
  const pre = (await info()).speed;
  await page.keyboard.down(' ');
  await sleep(400);
  s = (await info()).speed;
  await page.keyboard.up(' ');
  log('handbrake brakes hard', s < pre * 0.4, `speed ${pre.toFixed(0)} -> ${s.toFixed(0)} after 0.4s handbrake`);

  // --- Test 11: V toggles the 3D view (and 3D frames render cleanly) ---
  await page.evaluate(async () => (await import('./pkg/gt6.js')).debug_clear_heat());
  const vm = () =>
    page.evaluate(async () => (await import('./pkg/gt6.js')).debug_view_mode());
  await page.keyboard.press('v');
  await sleep(1500); // let several 3D frames render while coasting
  let m = await vm();
  log('V enters 3D mode', m === 1, `view_mode=${m}`);
  await page.keyboard.down('w');
  await sleep(1200); // drive in 3D (chase cam should follow)
  await page.keyboard.up('w');
  m = await vm();
  log('3D view survives driving', m === 1 && (await info()).speed > 0, `view_mode=${m}`);
  await page.screenshot({ path: '/tmp/gt6test/3d-mode.png' });
  await page.keyboard.press('v');
  await sleep(500);
  m = await vm();
  log('V returns to top-down', m === 0, `view_mode=${m}`);

  // --- Test 12: arrow-only driving in a known open park ---
  // Teleport to the center of the park block at (740, 1860), facing south:
  // ~280px of collision-free ground straight ahead, room to turn.
  if ((await info()).info[2]) { await page.keyboard.press('e'); await sleep(400); }
  await page.evaluate(async () => (await import('./pkg/gt6.js')).debug_teleport(740, 1860, Math.PI / 2));
  await sleep(200);
  const wrapAngle = (a) => {
    a = a % (2 * Math.PI);
    if (a > Math.PI) a -= 2 * Math.PI;
    if (a < -Math.PI) a += 2 * Math.PI;
    return a;
  };
  const dirOver = async (ms) => {
    const a = (await info()).info;
    await sleep(ms);
    const b = (await info()).info;
    return Math.atan2(b[1] - a[1], b[0] - a[0]);
  };

  // ArrowUp: accelerate straight (south).
  await page.keyboard.down('ArrowUp');
  await sleep(300);
  const sUp = (await info()).speed;
  const d1 = await dirOver(250);
  const straightErr = Math.abs(wrapAngle(d1 - Math.PI / 2));
  log('ArrowUp drives straight', sUp > 100 && straightErr < 0.15, `speed=${sUp.toFixed(0)}, off-axis=${(straightErr * 180 / Math.PI).toFixed(0)}\u00b0`);

  // Throttle-held turns (short arcs; south then east of the park are open
  // ground, so the car never hits a building).
  // ArrowRight curves the heading right.
  await page.keyboard.down('ArrowRight');
  const d2 = await dirOver(300);
  await page.keyboard.up('ArrowRight');
  const daR = wrapAngle(d2 - d1);
  log('ArrowRight steers right', daR > 0.12, `dheading=${(daR * 180 / Math.PI).toFixed(0)}\u00b0`);

  // ArrowLeft steers back left: after the turn, coasting straight must bring
  // the travel direction back to the original axis (it was swung right by
  // the ArrowRight phase).
  await page.keyboard.down('ArrowLeft');
  await sleep(300);
  await page.keyboard.up('ArrowLeft');
  const d4 = await dirOver(250);
  const daL = wrapAngle(d4 - d2);
  log('ArrowLeft steers back left', daL < -0.05, `dheading=${(daL * 180 / Math.PI).toFixed(0)}\u00b0`);

  // ArrowDown: re-accelerate, then brake hard.
  await page.keyboard.down('ArrowUp');
  await sleep(800);
  const preB = (await info()).speed;
  await page.keyboard.down('ArrowDown');
  await sleep(350);
  const postB = (await info()).speed;
  await page.keyboard.up('ArrowDown');
  await page.keyboard.up('ArrowUp');
  log('ArrowDown brakes hard', preB > 200 && postB < preB * 0.6, `${preB.toFixed(0)} -> ${postB.toFixed(0)} px/s`);
  await sleep(800); // roll to a stop
  await page.screenshot({ path: '/tmp/gt6test/arrows.png' });

  await page.screenshot({ path: '/tmp/gt6test/after-drive.png' });
  await sleep(800);
  await page.screenshot({ path: '/tmp/gt6test/final.png' });

  // --- JS errors ---
  const realErrors = errors.filter(
    (e) => !e.includes('The AudioContext was not allowed')
  );
  log('no page errors', realErrors.length === 0, realErrors.join(' | ') || 'clean');

  console.log('\n=== BROWSER TEST RESULTS ===');
  results.forEach((r) => console.log(r));
  const failed = results.filter((r) => r.startsWith('FAIL')).length;
  console.log(`\n${results.length - failed}/${results.length} passed`);
  await browser.close();
  process.exit(failed > 0 ? 1 : 0);
})().catch((e) => {
  console.error('TEST RUNNER ERROR:', e);
  process.exit(2);
});
