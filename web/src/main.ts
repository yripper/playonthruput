// The game loop. Four clocks run here:
// - every animation frame: sample input, predict the present, render;
// - every second: read the confirmed arena from chain storage;
// - every half second: send this player's new inputs as a transaction;
// - every block: crank the arena (every seated browser).

import './style.css';
import { type Session, openSession } from './chain';
import { ARENA_ADDRESS, MAX_BATCH } from './config';
import { createNetStats } from './debug';
import { Engine, H, type PlayerView, W } from './engine';
import { type FragEvent, type KillCamEvent, createEventFeed } from './events';
import { Controls } from './input';
import { fetchState, inclusionOffset, sampleClockOffset } from './rpc';

/** The engine's palette (engine/src/render.rs), for scoreboard swatches. */
const PALETTE = [
  [0, 0, 0], [40, 40, 48], [70, 56, 40], [150, 60, 40], [125, 48, 32], [120, 120, 110], [95, 95, 88], [60, 55, 50],
  [90, 35, 24], [40, 70, 80], [60, 110, 120], [65, 65, 60], [170, 30, 30], [230, 220, 120], [100, 15, 15], [240, 240, 240],
];
/** Stamp inputs slightly ahead of the estimated chain clock: late stamps can miss their tic, early ones only buffer. */
const CLOCK_LEAD_MS = 500;
/** Sends overlap (nonces are random), so an input waits at most this long to leave. */
const SEND_EVERY_MS = 250;
const POLL_STATE_MS = 1000;
const POLL_EVENTS_MS = 2000;
const CRANK_CHECK_MS = 500;
/** One block. */
const CRANK_EVERY_MS = 2000;
/** Without a new state for this long, browsers that aren't seated crank too (a join waits on one). */
const CRANK_RESCUE_MS = 5000;
const FEED_LEN = 12;
/** How long the kill cam stays up after a death is learned of. */
const DEATH_SCREEN_MS = 4500;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const els = {
  screen: $<HTMLDivElement>('screen'),
  view: $<HTMLCanvasElement>('view'),
  hud: $<HTMLDivElement>('hud'),
  hp: $<HTMLSpanElement>('hp'),
  frags: $<HTMLSpanElement>('frags'),
  lobby: $<HTMLDivElement>('lobby'),
  enter: $<HTMLButtonElement>('enter'),
  lobbyStatus: $<HTMLParagraphElement>('lobby-status'),
  death: $<HTMLDivElement>('death'),
  killcam: $<HTMLCanvasElement>('killcam'),
  killcamCaption: $<HTMLElement>('killcam-caption'),
  respawn: $<HTMLParagraphElement>('respawn'),
  youTic: $<HTMLSpanElement>('you-tic'),
  chainTic: $<HTMLSpanElement>('chain-tic'),
  chainMeta: $<HTMLSpanElement>('chain-meta'),
  gapFill: $<HTMLDivElement>('gap-fill'),
  board: $<HTMLOListElement>('board'),
  boardEmpty: $<HTMLParagraphElement>('board-empty'),
  feed: $<HTMLOListElement>('feed'),
  contract: $<HTMLSpanElement>('contract-address'),
  debug: $<HTMLPreElement>('debug'),
};
els.contract.textContent = ARENA_ADDRESS;

const engine = await Engine.load();
const viewCtx = els.view.getContext('2d')!;
const viewImage = viewCtx.createImageData(W, H);
const camCtx = els.killcam.getContext('2d')!;
const camImage = camCtx.createImageData(W, H);

let session: Session | null = null;
let mySlot = -1;
let wasSeated = false;
let followSlot = 0;
let haveState = false;
let stateHeight = 0;
let players: PlayerView[] = [];
const net = createNetStats(els.debug);

// Chain time = local time + offset. Two estimates:
// - from the newest visible block. Blocks reach the RPC 2.5-3.3 s after their
//   stamp on Futurenet, so this runs seconds slow; only spectators use it.
// - from this browser's own input transactions: the stamp of the block each
//   one landed in, minus when it was sent. That is the clock the crank that
//   drains those inputs runs on, so players stamp inputs by it.
//
// The game clock eases toward each new estimate instead of jumping: a jump
// forward leaves tics nobody sampled, a jump back stalls sampling, and either
// way the inputs the chain gets stop matching the screen.
const visibleSamples: number[] = [];
const landedSamples: number[] = [];
/** The offset the game clock runs on; eases toward `targetOffsetMs`. */
let offsetMs = 0;
let targetOffsetMs: number | null = null;
let easedAt = Date.now();
/** While easing, the game clock runs at most this much fast or slow. */
const CLOCK_EASE = 0.05;
let lastLandedSampleAt = 0;
const LANDED_SAMPLE_EVERY_MS = 3000;
const KEEP_SAMPLES = 9;

function updateOffset(): void {
  if (landedSamples.length) {
    const sorted = [...landedSamples].sort((a, b) => a - b);
    targetOffsetMs = sorted[sorted.length >> 1];
  } else if (visibleSamples.length) {
    // The newest block is stamped before we see it, so the largest sample is the closest.
    targetOffsetMs = Math.max(...visibleSamples);
  }
  // Nothing on screen depends on the clock yet, so it may jump.
  if (mySlot < 0 && targetOffsetMs !== null) offsetMs = targetOffsetMs;
}

function keep(samples: number[], value: number): void {
  samples.push(value);
  if (samples.length > KEEP_SAMPLES) samples.shift();
  updateOffset();
}

async function syncClock(): Promise<void> {
  try {
    keep(visibleSamples, await sampleClockOffset());
  } catch {
    // Keep the previous estimate; the next sample will retry.
  }
}

/** Samples the landed-block clock from an input transaction, once it has landed. */
async function sampleLanded(hash: string, sentAt: number): Promise<void> {
  for (let attempt = 0; attempt < 8; attempt++) {
    await new Promise((r) => setTimeout(r, 1000));
    try {
      const offset = await inclusionOffset(hash, sentAt);
      if (offset !== null) return keep(landedSamples, offset);
    } catch {
      // Try again; a missed sample only delays the estimate.
    }
  }
}

function chainNowMs(): number {
  const now = Date.now();
  if (targetOffsetMs !== null) {
    const step = (now - easedAt) * CLOCK_EASE;
    offsetMs += Math.max(-step, Math.min(step, targetOffsetMs - offsetMs));
  }
  easedAt = now;
  return now + offsetMs;
}

function presentTic(): number {
  const ms = chainNowMs() + CLOCK_LEAD_MS - engine.genesisMs;
  return Math.max(0, Math.floor((ms * engine.ticsPerSecond) / 1000));
}

// ---- inputs ---------------------------------------------------------------

const myInputs = new Map<number, number>();
let lastSampled = -1;
let sentThrough = -1;
/** Earliest tic of a send that failed, to resend from. */
let failedFrom = Infinity;

const controls = new Controls(els.view, (dir) => {
  const active = players.filter((p) => p.active);
  if (active.length === 0) return;
  const i = active.findIndex((p) => p.slot === followSlot);
  followSlot = active[(i + dir + active.length) % active.length].slot;
});

function sampleInputs(present: number): void {
  // A backgrounded tab stops animation frames; don't replay a burst on return.
  if (lastSampled < present - engine.ticsPerSecond) lastSampled = present - 1;
  for (let t = lastSampled + 1; t <= present; t++) myInputs.set(t, controls.sample());
  lastSampled = present;
}

function inputsFrom(start: number, end: number): Uint8Array {
  const out = new Uint8Array(Math.max(0, end - start + 1));
  for (let t = start; t <= end; t++) out[t - start] = myInputs.get(t) ?? 0;
  return out;
}

/**
 * Sends the inputs sampled since the last send, without waiting for earlier
 * sends to land: a send takes over a second, and an input that waits that
 * long to leave can reach the chain after its tic was simulated.
 */
async function sendInputs(): Promise<void> {
  if (!session || mySlot < 0) return;
  const from = Math.max(Math.min(sentThrough + 1, failedFrom), engine.confirmedTic);
  const to = Math.min(lastSampled, from + MAX_BATCH - 1);
  if (to < from) return;
  failedFrom = Infinity;
  sentThrough = Math.max(sentThrough, to);
  const sentAt = Date.now();
  try {
    const hash = await session.sendInputs(from, inputsFrom(from, to));
    if (sentAt - lastLandedSampleAt > LANDED_SAMPLE_EVERY_MS) {
      lastLandedSampleAt = sentAt;
      void sampleLanded(hash, sentAt);
    }
  } catch (err) {
    console.warn('send_inputs failed; resending', err);
    failedFrom = Math.min(failedFrom, from);
  }
  for (const t of myInputs.keys()) if (t < engine.confirmedTic - 10) myInputs.delete(t);
}

// ---- chain reads ----------------------------------------------------------

let feedPoll: Awaited<ReturnType<typeof createEventFeed>> | null = null;
const feed: FragEvent[] = [];
const cams: KillCamEvent[] = [];

async function pollState(): Promise<void> {
  try {
    const state = await fetchState();
    if (!state || state.height === stateHeight) return;
    const before = engine.confirmedTic;
    if (!engine.loadState(state.bytes)) return;
    if (engine.confirmedTic > before) {
      lastAdvanceAt = Date.now();
      if (haveState) net.advanced();
    }
    haveState = true;
    stateHeight = state.height;
    if (!feedPoll) feedPoll = await createEventFeed(Math.max(0, stateHeight - 30));
    if (session) mySlot = engine.slotOf(session.id);
    if (mySlot >= 0) checkPrediction();
    if (wasSeated && mySlot < 0) {
      showLobby('You went quiet for a minute and left the arena. Enter again to play.');
    }
    wasSeated = mySlot >= 0;
  } catch (err) {
    console.warn('state read failed', err);
  }
}

// Cranks that share a block simulate the same tics, so a kill can be reported
// more than once.
const seenKills = new Set<string>();
const firstReport = (e: { tic: number; killerSlot: number; victimSlot: number }, kind: string) => {
  const key = `${kind}:${e.tic}:${e.killerSlot}:${e.victimSlot}`;
  if (seenKills.has(key)) return false;
  seenKills.add(key);
  return true;
};

/**
 * Did the chain put this player where the browser predicted it would, at the
 * newly confirmed tic? A miss means the screen snaps (the readout's corrections).
 */
function checkPrediction(): void {
  const predicted = ownTrace.get(engine.confirmedTic);
  if (!predicted) return;
  const chain = engine.players(false)[mySlot];
  net.checked(Math.hypot(predicted.x - chain.x, predicted.y - chain.y) > 0.01);
}
/** The player's predicted position per tic, until the chain confirms it. */
const ownTrace = new Map<number, { x: number; y: number }>();

async function pollEvents(): Promise<void> {
  if (!feedPoll) return;
  try {
    const page = await feedPoll();
    const frags = page.frags.filter((f) => firstReport(f, 'frag'));
    const newCams = page.cams.filter((c) => firstReport(c, 'cam'));
    feed.unshift(...frags.reverse());
    feed.length = Math.min(feed.length, FEED_LEN);
    cams.push(...newCams);
    if (cams.length > 32) cams.splice(0, cams.length - 32);
    if (frags.length) renderFeed();
  } catch (err) {
    console.warn('event read failed', err);
  }
}

/** Wall time since a read state last moved the arena forward. */
let lastAdvanceAt = 0;

let lastCrankPeriod = -1;

/**
 * Every seated browser cranks once per block. Only one crank per block can
 * drain the input queue; the chain defers the others to the next block, so
 * the redundancy keeps every block cranked instead of leaving holes.
 */
async function maybeCrank(): Promise<void> {
  if (!session || !haveState) return;
  const period = Math.floor(chainNowMs() / CRANK_EVERY_MS);
  if (period === lastCrankPeriod) return;
  if (mySlot < 0 && Date.now() - lastAdvanceAt < CRANK_RESCUE_MS) return;
  lastCrankPeriod = period;
  net.crankSent();
  session.crank().catch((err) => console.warn('crank failed', err));
}

// ---- joining --------------------------------------------------------------

function showLobby(status: string): void {
  els.lobby.hidden = false;
  els.hud.hidden = true;
  els.enter.disabled = false;
  els.lobbyStatus.textContent = status;
}

function waitFor(cond: () => boolean, timeoutMs: number): Promise<boolean> {
  return new Promise((resolve) => {
    const start = Date.now();
    const timer = setInterval(() => {
      if (cond() || Date.now() - start > timeoutMs) {
        clearInterval(timer);
        resolve(cond());
      }
    }, 250);
  });
}

async function enter(): Promise<void> {
  els.enter.disabled = true;
  try {
    els.lobbyStatus.textContent = 'Making your session key…';
    session ??= await openSession();
    els.lobbyStatus.textContent = 'Getting test tokens from the Futurenet faucet…';
    await session.ensureFunds();
    els.lobbyStatus.textContent = 'Asking the arena for a seat…';
    const joinSentAt = Date.now();
    const joinHash = await session.join();
    // Set the clock from the join before taking a seat, so it doesn't have to
    // ease seconds while the player moves.
    const clockReady = sampleLanded(joinHash, joinSentAt);
    // The join only takes effect when a crank drains the queue.
    setTimeout(() => session?.crank().catch(() => {}), 2200);
    els.lobbyStatus.textContent = 'Waiting for the chain to seat you…';
    await clockReady;
    const seated = await waitFor(() => mySlot >= 0, 20_000);
    if (!seated) {
      const full = players.filter((p) => p.active).length >= engine.maxPlayers;
      showLobby(full ? 'All sixteen seats are taken. Watching until one opens.' : 'No seat yet. Enter again to retry.');
      return;
    }
    sentThrough = engine.confirmedTic - 1;
    lastSampled = presentTic() - 1;
    els.lobby.hidden = true;
    els.hud.hidden = false;
    els.view.focus();
  } catch (err) {
    console.error(err);
    showLobby(`Couldn't enter: ${err instanceof Error ? err.message : String(err)}. Enter again to retry.`);
  }
}

els.enter.addEventListener('click', () => void enter());

// ---- rendering ------------------------------------------------------------

function shortAddress(a: string): string {
  return `${a.slice(0, 4)}…${a.slice(-4)}`;
}

function cssColor(index: number): string {
  const [r, g, b] = PALETTE[index] ?? PALETTE[0];
  return `rgb(${r} ${g} ${b})`;
}

function renderBoard(): void {
  const active = players.filter((p) => p.active).sort((a, b) => b.frags - a.frags || a.deaths - b.deaths);
  els.boardEmpty.hidden = active.length > 0;
  els.board.replaceChildren(
    ...active.map((p) => {
      const li = document.createElement('li');
      li.classList.toggle('me', p.slot === mySlot);
      li.classList.toggle('dead', !p.alive);
      const swatch = document.createElement('span');
      swatch.className = 'swatch';
      swatch.style.background = cssColor(p.color);
      const name = document.createElement('span');
      name.className = 'name';
      name.textContent = p.slot === mySlot ? 'you' : `player ${p.slot + 1}`;
      const score = document.createElement('span');
      score.className = 'score';
      score.textContent = `${p.frags} / ${p.deaths}`;
      score.title = 'frags / deaths';
      li.append(swatch, name, score);
      return li;
    }),
  );
}

function renderFeed(): void {
  const me = session?.address;
  // Same names as the scoreboard; the address is kept in the tooltip.
  const who = (addr: string, slot: number) => (addr === me ? 'you' : `player ${slot + 1}`);
  els.feed.replaceChildren(
    ...feed.map((f) => {
      const li = document.createElement('li');
      li.textContent = `${who(f.killer, f.killerSlot)} fragged ${who(f.victim, f.victimSlot)}`;
      li.title = `${f.killer} fragged ${f.victim}`;
      const meta = document.createElement('span');
      meta.className = 'meta';
      meta.textContent = `block ${f.blockHeight}, tic ${f.tic}`;
      li.append(meta);
      return li;
    }),
  );
}

let shownCam: KillCamEvent | null = null;
// Other players' shots reach this browser seconds late, often after the
// 2-second respawn, so a death is noticed by the deaths counter going up
// rather than by catching the player dead.
let knownDeaths = -1;
let deathScreenUntil = 0;

function renderDeath(me: PlayerView, present: number): void {
  els.death.hidden = false;
  const secs = Math.max(0, (me.respawnAt - present) / engine.ticsPerSecond);
  els.respawn.textContent = !me.alive && secs > 0 ? `Respawning in ${secs.toFixed(1)} s` : 'Back in. Click to play.';
  const recent = cams.filter((c) => c.victimSlot === mySlot && c.tic > present - engine.simDelay * 4);
  const cam = recent.at(-1);
  if (cam && cam !== shownCam) {
    const rgba = engine.decodeKillCam(cam.frame);
    if (rgba) {
      camImage.data.set(rgba);
      camCtx.putImageData(camImage, 0, 0);
      els.killcamCaption.textContent = `What player ${cam.killerSlot + 1} saw. Rendered on-chain in block ${cam.blockHeight}, tx ${shortAddress(cam.transactionHash)}.`;
      shownCam = cam;
    }
  }
}

function renderLedger(present: number): void {
  if (!haveState) return;
  const confirmed = engine.confirmedTic;
  const behind = (present - confirmed) / engine.ticsPerSecond;
  els.youTic.textContent = `tic ${present}`;
  els.chainTic.textContent = `tic ${confirmed}`;
  // Nobody cranks an empty arena; say so instead of showing a growing gap.
  els.chainMeta.textContent =
    behind > 15
      ? `block ${stateHeight}, asleep until someone enters`
      : `block ${stateHeight}, ${behind.toFixed(1)} s behind you`;
  els.gapFill.style.width = `${Math.min(100, ((present - confirmed) / (engine.simDelay * 2)) * 100)}%`;
}

function frame(): void {
  requestAnimationFrame(frame);
  if (!haveState) return;
  const present = presentTic();
  const seated = mySlot >= 0 && els.lobby.hidden;
  const started = performance.now();

  if (seated) {
    sampleInputs(present);
    const start = engine.confirmedTic;
    engine.predict(mySlot, start, inputsFrom(start, present), present + 1);
  } else {
    // Spectators see the arena where everyone's inputs are already known.
    engine.predict(-1, 0, new Uint8Array(0), present - engine.rewind);
  }
  players = engine.players(true);

  if (!seated && !players[followSlot]?.active) {
    followSlot = players.find((p) => p.active)?.slot ?? 0;
  }
  const slot = seated ? mySlot : followSlot;
  viewImage.data.set(engine.render(slot, !seated));
  viewCtx.putImageData(viewImage, 0, 0);
  net.frame(performance.now() - started, (present - engine.confirmedTic) / engine.ticsPerSecond);
  if (seated) trackSnaps();

  if (seated) {
    const me = players[mySlot];
    els.hp.textContent = String(Math.max(0, me.hp));
    els.hp.classList.toggle('low', me.hp <= 30);
    els.frags.textContent = `${me.frags} frag${me.frags === 1 ? '' : 's'}`;
    if (knownDeaths >= 0 && me.deaths > knownDeaths) deathScreenUntil = Date.now() + DEATH_SCREEN_MS;
    knownDeaths = me.deaths;
    if (!me.alive || Date.now() < deathScreenUntil) {
      renderDeath(me, present);
    } else {
      els.death.hidden = true;
    }
  }
  renderLedger(present);
}

function trackSnaps(): void {
  const seen = new Map<number, { x: number; y: number }>();
  for (const p of players) {
    if (p.slot === mySlot) continue;
    const at = engine.seenPosition(p.slot);
    if (at) seen.set(p.slot, at);
  }
  const me = players[mySlot];
  ownTrace.set(engine.predictedTic, { x: me.x, y: me.y });
  for (const t of ownTrace.keys()) if (t < engine.confirmedTic) ownTrace.delete(t);
  net.track(engine.predictedTic, seen, me.alive ? { x: me.x, y: me.y } : null);
}

// Board updates on a slower beat than frames; it's text, not motion.
setInterval(renderBoard, 500);
setInterval(net.render, 250);

await syncClock();
setInterval(() => void syncClock(), 20_000);
// A returning player whose key is still seated goes straight back in.
if (localStorage.getItem('playonthruput.session-seed')) session = await openSession();
await pollState();
if (mySlot >= 0) {
  sentThrough = engine.confirmedTic - 1;
  lastSampled = presentTic() - 1;
  els.lobby.hidden = true;
  els.hud.hidden = false;
}
setInterval(() => void pollState(), POLL_STATE_MS);
setInterval(() => void pollEvents(), POLL_EVENTS_MS);
setInterval(() => void sendInputs(), SEND_EVERY_MS);
setInterval(() => void maybeCrank(), CRANK_CHECK_MS);
requestAnimationFrame(frame);
