// @name Better Shuffle
// @author community (Rhai port)
// @version 2.0.0
// @description Spotify-style shuffle that is not trapped in your search results, plus Smart Shuffle.
// @permissions shuffle, queue, search, library, notify

// Port of better-shuffle.js (Riff). Same modes and same settings.
// The state (`this`) keeps `mode` and `taste`; keys starting with `_` (the session) are not saved.
// To change the mode, edit default_mode() below: "classic", "spotify" or "smart".

fn default_mode() { "smart" }

fn modes() { ["classic", "spotify", "smart"] }

fn cfg() {
  #{
    expand_search: true,
    artists_to_expand: 4,
    per_artist: 10,
    max_expand: 50,
    smart_every: 3,
    smart_batch: 10,
    skip_sec: 30,
    toast_smart: true
  }
}

// ---------- keys ----------

fn artist_key(t) {
  if type_of(t) != "map" || t.artist == () { return "unknown"; }
  let key = "";
  for word in t.artist.to_string().to_lower().split() {
    if key != "" { key += " "; }
    key += word;
  }
  if key == "" { "unknown" } else { key }
}

fn tkey(t) {
  let platform = if t.platform == () { "" } else { t.platform.to_string() };
  let id = if t.id != () && t.id != "" {
    t.id.to_string()
  } else if t.url != () && t.url != "" {
    t.url
  } else {
    `${t.title}|${t.artist}`
  };
  platform + ":" + id
}

fn loose(t) {
  let title = if t.title == () { "" } else { t.title.to_string().to_lower() };
  title.trim();
  title + "::" + artist_key(t)
}

// ---------- ordering ----------

fn shuffled(list) {
  let a = list;
  let i = a.len() - 1;
  while i > 0 {
    let j = (random() * (i + 1).to_float()).floor().to_int();
    a.swap_items(i, j);
    i -= 1;
  }
  a
}

// Method on an array: `this` is the array.
fn swap_items(i, j) {
  let tmp = this[i];
  this[i] = this[j];
  this[j] = tmp;
}

fn is_bad(i, prev_key) {
  let k = artist_key(this[i]);
  let before = if i == 0 { prev_key } else { artist_key(this[i - 1]) };
  if k == before { return true; }
  i + 1 < this.len() && k == artist_key(this[i + 1])
}

fn fix_adjacent(list, prev_key) {
  let a = list;
  let n = a.len();
  for i in 0..n {
    let before = if i == 0 { prev_key } else { artist_key(a[i - 1]) };
    if artist_key(a[i]) != before { continue; }
    for j in (i + 1)..n {
      a.swap_items(i, j);
      if !a.is_bad(i, prev_key) && !a.is_bad(j, prev_key) { break; }
      a.swap_items(i, j);
    }
  }
  a
}

fn arrange(tracks, prev_key) {
  let groups = #{};
  let order = [];
  for t in tracks {
    let k = artist_key(t);
    if groups[k] == () {
      groups[k] = [];
      order.push(k);
    }
    groups[k].push(t);
  }
  let scored = [];
  for k in order {
    let s = shuffled(groups[k]);
    let n = s.len().to_float();
    for i in 0..s.len() {
      scored.push(#{ t: s[i], pos: (i.to_float() + random()) / n });
    }
  }
  scored.sort(|x, y| if x.pos < y.pos { -1 } else if x.pos > y.pos { 1 } else { 0 });
  let out = [];
  for x in scored { out.push(x.t); }
  fix_adjacent(out, prev_key)
}

// ---------- taste ----------

fn init() {
  if this.mode == () || !modes().contains(this.mode) { this.mode = default_mode(); }
  if this.taste == () { this.taste = #{ artists: #{}, skips: #{} }; }
}

fn affinity(t, favs) {
  let k = artist_key(t);
  let n = 0;
  for f in favs { if artist_key(f) == k { n += 1; } }
  let fav_boost = n.to_float() * 1.5;
  if fav_boost > 6.0 { fav_boost = 6.0; }
  let score = this.taste.artists[k];
  if score == () { score = 0; }
  let skips = this.taste.skips[tkey(t)];
  if skips == () { skips = 0; }
  score.to_float() + fav_boost - skips.to_float() * 1.5
}

// ---------- session ----------

fn new_session(ctx) {
  let cur = ctx.current;
  this._sid = if this._sid == () { 1 } else { this._sid + 1 };
  let s = #{
    id: this._sid,
    context: ctx.context,
    base: #{},
    pool: #{},
    order: [],
    played: #{},
    upcoming: [],
    smart_pool: [],
    smart_keys: #{},
    since_smart: 0,
    last_artist: if cur == () { "" } else { artist_key(cur) }
  };
  for t in ctx.queue {
    let k = tkey(t);
    s.base[k] = true;
    if s.pool[k] == () { s.order.push(k); }
    s.pool[k] = t;
  }
  let ck = if cur == () { "" } else { tkey(cur) };
  if ck != "" { s.played[ck] = true; }
  let rest = [];
  for t in ctx.queue { if ck == "" || tkey(t) != ck { rest.push(t); } }
  s.upcoming = arrange(rest, s.last_artist);
  this._s = s;
  // JS: enrich(S, ctx) without waiting. Here: done at the next track change (search is allowed there).
  this._enrich = #{ sid: s.id, context: ctx.context, queue: ctx.queue, current: cur };
}

fn session_valid(ctx) {
  let s = this._s;
  if s == () || s.context != ctx.context || s.base.len() == 0 { return false; }
  let have = #{};
  for t in ctx.queue { have[tkey(t)] = true; }
  let kept = 0;
  for k in s.base.keys() { if have[k] != () { kept += 1; } }
  kept.to_float() / s.base.len().to_float() >= 0.5
}

fn sync_session(ctx) {
  let cur = ctx.current;
  if cur != () {
    let ck = tkey(cur);
    this._s.played[ck] = true;
    this._s.upcoming = this._s.upcoming.filter(|t| tkey(t) != ck);
  }
  for t in ctx.queue {
    let k = tkey(t);
    if this._s.pool[k] != () { continue; }
    this._s.pool[k] = t;
    this._s.order.push(k);
    let pos = (random() * (this._s.upcoming.len() + 1).to_float()).floor().to_int();
    this._s.upcoming.insert(pos, t);
  }
}

fn refill(ctx) {
  let cur = ctx.current;
  let ck = if cur == () { "" } else { tkey(cur) };
  this._s.played = #{};
  if ck != "" { this._s.played[ck] = true; }
  let all = [];
  for k in this._s.order { if k != ck { all.push(this._s.pool[k]); } }
  this._s.upcoming = arrange(all, this._s.last_artist);
  if this.mode == "smart" { this._recommend = this._s.id; }
}

fn add_to_session(sid, added, is_smart) {
  if this._s == () || this._s.id != sid { return; }
  for t in added {
    let k = tkey(t);
    if this._s.pool[k] != () { continue; }
    this._s.pool[k] = t;
    this._s.order.push(k);
    if is_smart {
      this._s.smart_keys[k] = true;
      this._s.smart_pool.push(t);
    } else {
      let pos = (random() * (this._s.upcoming.len() + 1).to_float()).floor().to_int();
      this._s.upcoming.insert(pos, t);
    }
  }
  if !is_smart { this._s.upcoming = fix_adjacent(this._s.upcoming, this._s.last_artist); }
}

// ---------- enrichment (search and vibe go through Riff) ----------

fn expand_search(sid, job) {
  let names = #{};
  let counts = #{};
  let order = [];
  for t in job.queue {
    let k = artist_key(t);
    if k == "" || k == "unknown" { continue; }
    if counts[k] == () {
      counts[k] = 0;
      names[k] = t.artist;
      order.push(k);
    }
    counts[k] += 1;
  }
  let ranked = [];
  for k in order { ranked.push(#{ k: k, n: counts[k] }); }
  ranked.sort(|a, b| b.n - a.n);
  let found = [];
  for entry in ranked.extract(0, cfg().artists_to_expand) {
    let k = entry.k;
    for t in search(names[k], #{ limit: cfg().per_artist }) {
      let a = artist_key(t);
      if a.contains(k) || k.contains(a) { found.push(t); }
    }
  }
  found = shuffled(found).extract(0, cfg().max_expand);
  this.add_to_session(sid, queue_append(found), false);
}

fn recommend(sid, current) {
  if this._s == () || this._s.id != sid { return; }
  let pool = [];
  for k in this._s.order { pool.push(this._s.pool[k]); }
  let seeds = shuffled(pool).extract(0, 12);
  if current != () { seeds.insert(0, current); }
  let ids = [];
  let keys = [];
  for t in pool {
    if t.id != () && t.id != "" && ids.len() < 120 { ids.push(t.id.to_string()); }
    if keys.len() < 120 { keys.push(loose(t)); }
  }
  let recs = vibe(#{
    history: seeds,
    favorites: favorites().extract(0, 10),
    limit: cfg().smart_batch,
    avoid: #{ ids: ids, keys: keys, artists: [] }
  });
  let fresh = [];
  for t in recs {
    if t != () && t.title != () && this._s.pool[tkey(t)] == () { fresh.push(t); }
  }
  this.add_to_session(sid, queue_append(fresh), true);
}

// ---------- picking ----------

fn take_smart() {
  if this._s.smart_pool.len() == 0 { return (); }
  let favs = favorites();
  let best = -1;
  let best_score = 0.0;
  for i in 0..this._s.smart_pool.len() {
    let t = this._s.smart_pool[i];
    if artist_key(t) == this._s.last_artist { continue; }
    let score = this.affinity(t, favs) + random() * 1.5;
    if best < 0 || score > best_score {
      best_score = score;
      best = i;
    }
  }
  if best < 0 { return (); }
  this._s.smart_pool.remove(best)
}

fn take_user() {
  let index = -1;
  for i in 0..this._s.upcoming.len() {
    if artist_key(this._s.upcoming[i]) != this._s.last_artist {
      index = i;
      break;
    }
  }
  if index < 0 { index = 0; }
  this._s.upcoming.remove(index)
}

// ---------- entry points ----------

fn on_ready() { this.init(); }

fn shuffle_pick(ctx) {
  this.init();
  if this.mode == "classic" || ctx.queue.len() == 0 { return (); }
  if ctx.queue.len() == 1 { return ctx.queue[0]; }
  if !this.session_valid(ctx) { this.new_session(ctx); } else { this.sync_session(ctx); }
  if this._s.upcoming.len() == 0 && this._s.smart_pool.len() == 0 { this.refill(ctx); }

  let next = ();
  if this.mode == "smart" && this._s.since_smart >= cfg().smart_every {
    next = this.take_smart();
    if next != () { this._s.since_smart = 0; }
  }
  if next == () && this._s.upcoming.len() > 0 {
    next = this.take_user();
    this._s.since_smart += 1;
  }
  if next == () { next = this.take_smart(); }
  if next == () { return (); }

  this._s.played[tkey(next)] = true;
  this._s.last_artist = artist_key(next);
  next
}

fn on_trackchange(track) {
  this.init();
  if cfg().toast_smart && this._s != () && track != () && this._s.smart_keys[tkey(track)] != () {
    notify("Smart shuffle pick: " + track.title, "info");
  }
  let job = this._enrich;
  if job != () {
    this._enrich = ();
    if cfg().expand_search && job.context == "search" { this.expand_search(job.sid, job); }
    if this.mode == "smart" { this.recommend(job.sid, job.current); }
  }
  let sid = this._recommend;
  if sid != () {
    this._recommend = ();
    this.recommend(sid, track);
  }
}

fn stop() {
  this._s = ();
  this._enrich = ();
  this._recommend = ();
}
