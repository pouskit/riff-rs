// Drives Riff (Electron) by the Chromium debugging protocol, for the CPU scenes. Node >= 22 (WebSocket).
//   node riff-driver.mjs start <port> <scene> : plays the first local track, prepares the scene, prints the state.
//   node riff-driver.mjs check <port> <png>   : prints the state, writes a screenshot.
import { writeFileSync } from 'node:fs';

const [mode, port, arg] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function connect() {
  for (let i = 0; i < 120; i++) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = pages.find((p) => p.type === 'page' && /index\.html/.test(p.url));
      if (page) return new WebSocket(page.webSocketDebuggerUrl);
    } catch { /* not listening yet */ }
    await sleep(500);
  }
  throw new Error('no debugging target');
}

const ws = await connect();
await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
let nextId = 0;
const waiting = new Map();
ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  if (waiting.has(message.id)) { waiting.get(message.id)(message); waiting.delete(message.id); }
};
const send = (method, params = {}) => new Promise((resolve) => {
  const id = ++nextId;
  waiting.set(id, resolve);
  ws.send(JSON.stringify({ id, method, params }));
});
const evaluate = async (expression) => {
  const reply = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (reply.result?.exceptionDetails) throw new Error(JSON.stringify(reply.result.exceptionDetails));
  return reply.result?.result?.value;
};
const until = async (expression, what, tries = 150) => {
  for (let i = 0; i < tries; i++) {
    if (await evaluate(expression)) return;
    await sleep(200);
  }
  throw new Error(`timeout: ${what}`);
};

const STATE = `JSON.stringify({
  hidden: document.hidden,
  size: [innerWidth, innerHeight],
  time: (document.querySelector('audio') || {}).currentTime,
  paused: (document.querySelector('audio') || {}).paused,
  title: (document.getElementById('right-panel-title') || {}).textContent,
  panelOpen: !document.getElementById('right-panel').classList.contains('hidden'),
  lines: document.querySelectorAll('.lyric-line').length,
  words: document.querySelectorAll('.lyric-word').length,
  activeWords: document.querySelectorAll('.lyric-word.active-word').length,
  gradient: document.querySelectorAll('.gradient-line').length,
  coverBg: document.body.classList.contains('cover-bg-on'),
  m3c: (() => { const c = document.getElementById('m3c-canvas'); return !!c && c.offsetParent !== null; })(),
  barViz: (() => { const c = document.getElementById('visualizer-container'); return !!c && c.style.display !== 'none'; })(),
})`;

if (mode === 'start') {
  const scene = arg;
  await until(`!!document.getElementById('nav-local') && !!document.getElementById('btn-toggle-lyrics-panel')`, 'interface');
  await sleep(4000);
  const wantsPanel = scene.startsWith('b') || scene === 'e';
  if (!wantsPanel) await evaluate(`document.getElementById('btn-toggle-lyrics-panel').click()`);
  await evaluate(`document.getElementById('nav-local').click()`);
  await until(`document.querySelectorAll('#local-tracks-list .track-row').length > 0`, 'local track row');
  await sleep(1500);
  await evaluate(`(() => { const r = document.querySelector('#local-tracks-list .track-row'); r.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); r.click(); })()`);
  await until(`(() => { const a = document.querySelector('audio'); return !!a && !a.paused && a.currentTime > 8; })()`, 'lecture', 150);
  console.log(await evaluate(STATE));
} else {
  console.log(await evaluate(STATE));
  // A highlight that really moves: the most words seen active, and the distinct words, over 3 s.
  let activeMax = 0;
  const activeWords = new Set();
  for (let i = 0; i < 30; i++) {
    const seen = JSON.parse(await evaluate(`JSON.stringify([...document.querySelectorAll('.lyric-word.active-word')].map((w) => w.textContent))`));
    activeMax = Math.max(activeMax, seen.length);
    seen.forEach((w) => activeWords.add(w));
    await sleep(100);
  }
  const colors = await evaluate(`JSON.stringify((() => {
    const color = (el) => (el ? getComputedStyle(el).color : null);
    const line = document.querySelector('.lyric-line.active');
    return {
      activeWord: color(document.querySelector('.lyric-word.active-word')),
      pastWord: color(line && line.querySelector('.lyric-word.past-word')),
      plainWord: color(line && line.querySelector('.lyric-word:not(.active-word):not(.past-word)')),
    };
  })())`);
  console.log(JSON.stringify({ activeMax, activeWords: [...activeWords] }), colors);
  const shot = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync(arg, Buffer.from(shot.result.data, 'base64'));
}
ws.close();
