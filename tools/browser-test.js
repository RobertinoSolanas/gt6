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
    if (m.type() === 'error') errors.push('console: ' + m.text());
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
