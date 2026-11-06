// A netcode readout for checking lag and snaps: toggle it with the ` key, or
// open the page with ?debug. The numbers are also on window.netStats, so a
// headless test can read them.

/** Samples kept per list: about a minute and a half of frames. */
const KEEP = 6000;
/** One frame covers a few tics at most, and a tic moves 0.1 units, so a bigger jump is a correction. */
const SNAP_UNITS = 0.5;

export interface NetStats {
  /** Predict + render time per frame, ms. */
  frameMs: number[];
  /** Wall time between confirmed states that moved the arena forward, ms. */
  stateGapsMs: number[];
  /** How far the confirmed arena trails the screen, seconds, sampled per frame. */
  behindS: number[];
  opponentSnaps: number;
  ownSnaps: number;
  cranksSent: number;
}

type Point = { x: number; y: number };

export function createNetStats(panel: HTMLElement) {
  const stats: NetStats = {
    frameMs: [],
    stateGapsMs: [],
    behindS: [],
    opponentSnaps: 0,
    ownSnaps: 0,
    cranksSent: 0,
  };
  (window as unknown as { netStats: NetStats }).netStats = stats;
  panel.hidden = !new URLSearchParams(location.search).has('debug');
  addEventListener('keydown', (e) => {
    if (e.code === 'Backquote') panel.hidden = !panel.hidden;
  });

  let lastAdvanceAt = 0;
  const lastSeen = new Map<number, Point>();
  let lastOwn: Point | null = null;

  const push = (list: number[], value: number) => {
    list.push(value);
    if (list.length > KEEP) list.shift();
  };
  const moved = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y) > SNAP_UNITS;

  return {
    stats,

    frame(ms: number, behindS: number): void {
      push(stats.frameMs, ms);
      push(stats.behindS, behindS);
    },

    /** Call when a newly read state moved the confirmed tic forward. */
    advanced(): void {
      const now = performance.now();
      if (lastAdvanceAt) push(stats.stateGapsMs, now - lastAdvanceAt);
      lastAdvanceAt = now;
    },

    /** Counts frame-to-frame jumps: opponents where they're drawn, and the player's own position. */
    track(seen: Map<number, Point>, own: Point | null): void {
      for (const [slot, p] of seen) {
        const before = lastSeen.get(slot);
        if (before && moved(before, p)) stats.opponentSnaps++;
      }
      lastSeen.clear();
      for (const [slot, p] of seen) lastSeen.set(slot, p);
      if (own && lastOwn && moved(lastOwn, own)) stats.ownSnaps++;
      lastOwn = own;
    },

    crankSent(): void {
      stats.cranksSent++;
    },

    render(): void {
      if (panel.hidden) return;
      const avg = (l: number[]) => (l.length ? l.reduce((a, b) => a + b, 0) / l.length : 0);
      const worst = (l: number[]) => (l.length ? Math.max(...l) : 0);
      panel.textContent = [
        `chain behind  ${avg(stats.behindS).toFixed(1)} s avg, ${worst(stats.behindS).toFixed(1)} s worst`,
        `new state     every ${(avg(stats.stateGapsMs) / 1000).toFixed(1)} s, worst ${(worst(stats.stateGapsMs) / 1000).toFixed(1)} s`,
        `frame         ${avg(stats.frameMs).toFixed(1)} ms avg, ${worst(stats.frameMs).toFixed(1)} ms worst`,
        `snaps         opponents ${stats.opponentSnaps}, you ${stats.ownSnaps}`,
        `cranks sent   ${stats.cranksSent}`,
      ].join('\n');
    },
  };
}
