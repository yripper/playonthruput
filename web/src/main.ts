// The game loop. Four clocks run here:
// - every animation frame: sample input, predict the present, render;
// - every second: read the confirmed arena from chain storage;
// - every half second: send this player's new inputs as a transaction;
// - every block or so: crank the arena (whoever's browser lands it first).

import './style.css';
import { type Session, openSession } from './chain';
import { ARENA_ADDRESS, MAX_BATCH } from './config';
import { Engine, H, type PlayerView, W } from './engine';
import { type FragEvent, type KillCamEvent, createEventFeed } from './events';
import { Controls } from './input';
import { fetchState, sampleClockOffset } from './rpc';

/** The engine's palette (engine/src/render.rs), for scoreboard swatches. */
const PALETTE = [
  [0, 0, 0], [40, 40, 48], [70, 56, 40], [150, 60, 40], [125, 48, 32], [120, 120, 110], [95, 95, 88], [60, 55, 50],
  [90, 35, 24], [40, 70, 80], [60, 110, 120], [65, 65, 60], [170, 30, 30], [230, 220, 120], [100, 15, 15], [240, 240, 240],
];
/** Stamp inputs slightly ahead of the estimated chain clock: late stamps can miss their tic, early ones only buffer. */
const CLOCK_LEAD_MS = 500;
const SEND_EVERY_MS = 500;
const POLL_STATE_MS = 1000;
const POLL_EVENTS_MS = 2000;
const CRANK_EVERY_MS = 2000;
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

// Chain time = local time + offset. The newest block is stamped a little
// before we see it, so the largest sample is the closest estimate.
const offsetSamples: number[] = [];
let offsetMs = 0;

async function syncClock(): Promise<void> {
  try {
    offsetSamples.push(await sampleClockOffset());
    if (offsetSamples.length > 6) offsetSamples.shift();
    offsetMs = Math.max(...offsetSamples);
  } catch {
    // Keep the previous estimate; the next sample will retry.
  }
}

function presentTic(): number {
  const ms = Date.now() + offsetMs + CLOCK_LEAD_MS - engine.genesisMs;
  return Math.max(0, Math.floor((ms * engine.ticsPerSecond) / 1000));
}

// ---- inputs ---------------------------------------------------------------

const myInputs = new Map<number, number>();
let lastSampled = -1;
let sentThrough = -1;
let sending = false;

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

async function sendInputs(): Promise<void> {
  if (!session || mySlot < 0 || sending) return;
  const from = Math.max(sentThrough + 1, engine.confirmedTic);
  const to = Math.min(lastSampled, from + MAX_BATCH - 1);
  if (to < from) return;
  sending = true;
  try {
    await session.sendInputs(from, inputsFrom(from, to));
    sentThrough = to;
  } catch (err) {
    console.warn('send_inputs failed; retrying next round', err);
  } finally {
    sending = false;
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
    if (!engine.loadState(state.bytes)) return;
    haveState = true;
    stateHeight = state.height;
    if (!feedPoll) feedPoll = await createEventFeed(Math.max(0, stateHeight - 30));
    if (session) mySlot = engine.slotOf(session.id);
    if (wasSeated && mySlot < 0) {
      showLobby('You went quiet for a minute and left the arena. Enter again to play.');
    }
    wasSeated = mySlot >= 0;
  } catch (err) {
    console.warn('state read failed', err);
  }
}

async function pollEvents(): Promise<void> {
  if (!feedPoll) return;
  try {
    const { frags, cams: newCams } = await feedPoll();
    feed.unshift(...frags.reverse());
    feed.length = Math.min(feed.length, FEED_LEN);
    cams.push(...newCams);
    if (cams.length > 32) cams.splice(0, cams.length - 32);
    if (frags.length) renderFeed();
  } catch (err) {
    console.warn('event read failed', err);
  }
}

/** Cranks every block or so. With many players, each cranks with 1/n odds, unless the arena is falling behind. */
async function maybeCrank(): Promise<void> {
  if (!session || !haveState) return;
  const lag = presentTic() - engine.confirmedTic;
  const active = Math.max(1, players.filter((p) => p.active).length);
  if (lag > engine.simDelay + 45 || Math.random() < 1 / active) {
    session.crank().catch((err) => console.warn('crank failed', err));
  }
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
    await session.join();
    // The join only takes effect when a crank drains the queue.
    setTimeout(() => session?.crank().catch(() => {}), 2200);
    els.lobbyStatus.textContent = 'Waiting for the chain to seat you…';
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

// Board updates on a slower beat than frames; it's text, not motion.
setInterval(renderBoard, 500);

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
setInterval(() => void maybeCrank(), CRANK_EVERY_MS + Math.random() * 400);
requestAnimationFrame(frame);
