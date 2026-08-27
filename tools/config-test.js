// Headless config-page test:
//   - ESC opens the config page (and the world freezes)
//   - arrow keys move the selection, pressing a key rebinds it
//   - ENTER saves config.ini (downloads the file)
//   - applying a hand-written config.ini changes the live bindings
// Run with the game served at http://localhost:8090/ (see browser-test.js).
const puppeteer = require('puppeteer-core');
const fs = require('fs');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (name, ok, extra = '') => {
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${ok ? '' : `  ${extra}`}`);
  if (!ok) failures++;
};
(async () => {
  const browser = await puppeteer.launch({
    headless: 'new',
    executablePath: '/usr/bin/google-chrome',
    args: ['--no-sandbox', '--disable-gpu', '--autoplay-policy=no-user-gesture-required', '--window-size=1280,800'],
    defaultViewport: { width: 1280, height: 800 },
  });
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push('pageerror: ' + e.message));
  page.on('console', (m) => {
    // The boot-time config.ini probe 404s when no config.ini is checked in —
    // the generic "Failed to load resource ... 404" line can't name the URL,
    // so ignore bare 404s.
    if (m.type() === 'error' && !/404/.test(m.text())) errors.push('console: ' + m.text());
  });

  await page.goto('http://localhost:8090/', { waitUntil: 'networkidle0' });
  await page.evaluate(() => import('./pkg/gt6.js'));
  await sleep(1500); // let the game boot & run

  const ini = () => page.evaluate(async () => (await import('./pkg/gt6.js')).debug_config_ini());
  const cfgOpen = () => page.evaluate(async () => (await import('./pkg/gt6.js')).debug_config_open());
  const player = () => page.evaluate(async () => Array.from((await import('./pkg/gt6.js')).debug_player_info()));
  const dragon = () => page.evaluate(async () => Array.from((await import('./pkg/gt6.js')).debug_dragon()));

  // --- Default config ---
  const ini0 = await ini();
  check('default config exposes config.ini text', ini0.includes('[movement]') && /forward = w/m.test(ini0), ini0.slice(0, 120));
  check('config page starts closed', (await cfgOpen()) === 0);

  // --- ESC opens the page, world freezes ---
  await page.keyboard.press('Escape');
  await sleep(300);
  check('ESC opens the config page', (await cfgOpen()) === 1);
  await page.screenshot({ path: '/tmp/gt6_config.png' });
  const p0 = await player();
  await page.keyboard.down('w'); // held while the page is open: must do nothing
  await sleep(1000);
  await page.keyboard.up('w');
  const p1 = await player();
  check(
    'world is frozen while the config page is open',
    Math.abs(p0[0] - p1[0]) < 1e-6 && Math.abs(p0[1] - p1[1]) < 1e-6,
    `moved ${Math.abs(p0[0] - p1[0])},${Math.abs(p0[1] - p1[1])}`
  );

  // --- Rebind SUMMON DRAGON (row 13) to T: 13x down from the top row ---
  for (let i = 0; i < 13; i++) {
    await page.keyboard.press('ArrowDown');
    await sleep(50); // one key press per frame, so each moves the selection
  }
  await page.keyboard.press('t');
  await sleep(200);
  const ini1 = await ini();
  const line = ini1.split('\n').find((l) => l.includes('summon_dragon')) || '(missing)';
  check('pressing T rebinds SUMMON DRAGON', /summon_dragon = t/m.test(ini1), line);

  // --- ESC closes the page, T now summons the dragon ---
  await page.keyboard.press('Escape');
  await sleep(200);
  check('ESC closes the config page', (await cfgOpen()) === 0);
  await page.keyboard.down('t');
  await sleep(100);
  await page.keyboard.up('t');
  await sleep(600);
  const d = await dragon();
  check('T (rebound) summons + mounts the dragon', d[7] === 1, JSON.stringify(d));
  await page.screenshot({ path: '/tmp/gt6_config_dragon.png' });

  // --- E still unmounts (it was not rebound) ---
  await page.keyboard.down('e');
  await sleep(100);
  await page.keyboard.up('e');
  await sleep(400);
  check('E still unmounts the dragon', (await dragon())[7] === 0);

  // --- ENTER saves config.ini (anchor download, caught via CDP) ---
  const dlDir = '/tmp/gt6_download';
  fs.rmSync(dlDir, { recursive: true, force: true });
  fs.mkdirSync(dlDir, { recursive: true });
  const cdp = await page.target().createCDPSession();
  await cdp.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: dlDir, eventsEnabled: true });

  await page.keyboard.press('Escape'); // open the page
  await sleep(200);
  await page.keyboard.press('Enter'); // save
  await sleep(1500);
  const files = fs.readdirSync(dlDir);
  const iniFile = files.find((f) => f.endsWith('config.ini'));
  check('ENTER downloads a config.ini file', !!iniFile, `files: ${files.join(',')}`);
  if (iniFile) {
    const saved = fs.readFileSync(`${dlDir}/${iniFile}`, 'utf8');
    check('saved config.ini contains the rebind', /summon_dragon = t/m.test(saved), saved.slice(0, 400));
  }

  // --- Apply a hand-written config.ini: the live bindings change ---
  await page.keyboard.press('Escape'); // close the page (it is still open)
  await sleep(200);
  check('page closed after save', (await cfgOpen()) === 0);
  const custom = [
    '[movement]',
    'forward = k',
    'back = j',
    '[mouse]',
    'brake_button = lmb',
    'sensitivity = 2.00',
    '[specials]',
    'summon_airplane = b',
  ].join('\n');
  const applied = await page.evaluate(async (t) => (await import('./pkg/gt6.js')).debug_config_apply(t), custom);
  check('applying config.ini text works', applied === true);
  const ini2 = await ini();
  check('applied forward = k', /forward = k/m.test(ini2));
  check('applied sensitivity = 2.00', /sensitivity = 2\.00/m.test(ini2));
  check('applied brake_button = lmb', /brake_button = lmb/m.test(ini2));

  // K now drives the car (on a known open stretch, like browser-test.js).
  await page.evaluate(async () => (await import('./pkg/gt6.js')).debug_teleport(740, 1860, Math.PI / 2));
  await sleep(300);
  const c0 = await player();
  await page.keyboard.down('k');
  await sleep(700);
  await page.keyboard.up('k');
  const c1 = await player();
  check('K (rebound forward) drives the car', c1[0] !== c0[0] || c1[1] !== c0[1], JSON.stringify([c0, c1]));

  // B now summons the airplane.
  await page.keyboard.down('b');
  await sleep(100);
  await page.keyboard.up('b');
  await sleep(800);
  const alt = await page.evaluate(async () => (await import('./pkg/gt6.js')).debug_player_alt());
  check('B (rebound summon-airplane) takes the air', alt > 0, `alt=${alt}`);
  await page.screenshot({ path: '/tmp/gt6_config_plane.png' });

  check('no page/console errors', errors.length === 0, errors.join(' | ').slice(0, 400));

  await browser.close();
  console.log(failures === 0 ? '\nCONFIG TEST PASSED' : `\nCONFIG TEST FAILED (${failures})`);
  process.exit(failures === 0 ? 0 : 1);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
