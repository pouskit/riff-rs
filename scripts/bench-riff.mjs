// Probes of Riff (Electron) by the Chromium debugging protocol, for scripts\bench.ps1. Node >= 22 (WebSocket).
//   node bench-riff.mjs startup <port>                               : prints the paint timings of index.html as JSON.
//   node bench-riff.mjs search <port> <query|q1|q2|...> <trials> [platform] : times the page's own /api/search request.
//     Queries separated by '|' give one trial per query (distinct queries, to get past a server cache).
const [mode, port, ...rest] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function connect() {
  for (let i = 0; i < 200; i++) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = pages.find((p) => p.type === 'page' && /index\.html/.test(p.url));
      if (page) return new WebSocket(page.webSocketDebuggerUrl);
    } catch { /* not listening yet */ }
    await sleep(100);
  }
  throw new Error('no debugging target (index.html)');
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

if (mode === 'startup') {
  // First paint of the page: timeOrigin is the Unix time (ms) of the navigation start.
  const expression = `JSON.stringify({
    origin: performance.timeOrigin,
    fcp: (performance.getEntriesByName('first-contentful-paint')[0] || {}).startTime ?? null,
    fp: (performance.getEntriesByName('first-paint')[0] || {}).startTime ?? null,
    dcl: (performance.getEntriesByType('navigation')[0] || {}).domContentLoadedEventEnd ?? null,
    load: (performance.getEntriesByType('navigation')[0] || {}).loadEventEnd ?? null,
    url: location.pathname.split('/').pop(),
  })`;
  let last = null;
  for (let i = 0; i < 100; i++) {
    last = JSON.parse(await evaluate(expression));
    if (last.fcp !== null) break;
    await sleep(100);
  }
  console.log(JSON.stringify(last));
} else if (mode === 'search') {
  const [queryArg, trials, platform = 'ytmusic'] = rest;
  const queries = queryArg.includes('|') ? queryArg.split('|') : null;
  const port2 = await evaluate(`window.electronAPI.getServerPort()`);
  const expressionFor = (query) => `(async () => {
    const url = 'http://127.0.0.1:${port2}/api/search?q=' + encodeURIComponent(${JSON.stringify(query)}) + '&platform=${platform}&limit=12&offset=0';
    const t0 = performance.now();
    const res = await fetch(url);
    const data = await res.json();
    return JSON.stringify({ ms: performance.now() - t0, status: res.status, success: !!data.success, tracks: (data.tracks || []).length, error: data.error || null });
  })()`;
  const out = [];
  const count = queries ? queries.length : Number(trials);
  for (let i = 0; i < count; i++) {
    out.push(JSON.parse(await evaluate(expressionFor(queries ? queries[i] : queryArg))));
    await sleep(500);
  }
  console.log(JSON.stringify(out));
} else {
  throw new Error('unknown mode');
}
ws.close();
process.exit(0);
