// Typed wrapper over the engine's wasm exports (wasm/src/lib.rs).

import wasmUrl from './engine.wasm?url';

interface Exports {
  memory: WebAssembly.Memory;
  state_ptr(): number;
  state_len(): number;
  inputs_ptr(): number;
  id_ptr(): number;
  rgba_ptr(): number;
  killcam_ptr(): number;
  killcam_rgba_ptr(): number;
  players_ptr(): number;
  constant(which: number): number;
  load_state(): number;
  confirmed_tic(): number;
  predicted_tic(): number;
  genesis_ms(): number;
  slot_of(): number;
  predict(slot: number, start: number, len: number, target: number): void;
  snapshot(which: number): void;
  render(slot: number, spectate: number): void;
  decode_killcam(len: number): number;
  seen_x(slot: number): number;
  seen_y(slot: number): number;
}

export const W = 160;
export const H = 100;

export interface PlayerView {
  slot: number;
  active: boolean;
  alive: boolean;
  x: number;
  y: number;
  angle: number;
  hp: number;
  frags: number;
  deaths: number;
  respawnAt: number;
  color: number;
}

export class Engine {
  readonly simDelay: number;
  readonly rewind: number;
  readonly ticsPerSecond: number;
  readonly maxPlayers: number;
  readonly respawnTics: number;
  readonly maxHp: number;
  readonly inputsCap: number;
  private readonly playerFields: number;

  private readonly x: Exports;

  private constructor(x: Exports) {
    this.x = x;
    this.simDelay = x.constant(0);
    this.rewind = x.constant(1);
    this.ticsPerSecond = x.constant(2);
    this.maxPlayers = x.constant(3);
    this.respawnTics = x.constant(4);
    this.maxHp = x.constant(5);
    this.inputsCap = x.constant(6);
    this.playerFields = x.constant(7);
  }

  static async load(): Promise<Engine> {
    const { instance } = await WebAssembly.instantiateStreaming(fetch(wasmUrl));
    return new Engine(instance.exports as unknown as Exports);
  }

  private bytes(ptr: number, len: number): Uint8Array {
    return new Uint8Array(this.x.memory.buffer, ptr, len);
  }

  /** Loads the concatenated state chunks. False if they don't decode. */
  loadState(stored: Uint8Array): boolean {
    this.bytes(this.x.state_ptr(), this.x.state_len()).set(stored.subarray(0, this.x.state_len()));
    return this.x.load_state() === 0;
  }

  get confirmedTic(): number {
    return this.x.confirmed_tic();
  }

  get genesisMs(): number {
    return this.x.genesis_ms();
  }

  slotOf(id: Uint8Array): number {
    this.bytes(this.x.id_ptr(), 32).set(id);
    return this.x.slot_of();
  }

  /** Replays `inputs` (from tic `start`) for `slot` on top of the confirmed state, up to `target`. */
  predict(slot: number, start: number, inputs: Uint8Array, target: number): void {
    const len = Math.min(inputs.length, this.inputsCap);
    this.bytes(this.x.inputs_ptr(), len).set(inputs.subarray(0, len));
    this.x.predict(slot, start, len, target);
  }

  /** Where the player sees `slot` drawn, or null when it isn't drawn. */
  seenPosition(slot: number): { x: number; y: number } | null {
    const x = this.x.seen_x(slot);
    if (x === -0x80000000) return null;
    return { x: x / 65536, y: this.x.seen_y(slot) / 65536 };
  }

  players(predicted: boolean): PlayerView[] {
    this.x.snapshot(predicted ? 1 : 0);
    const raw = new Int32Array(this.x.memory.buffer, this.x.players_ptr(), this.maxPlayers * this.playerFields);
    const out: PlayerView[] = [];
    for (let s = 0; s < this.maxPlayers; s++) {
      const r = raw.subarray(s * this.playerFields, (s + 1) * this.playerFields);
      out.push({
        slot: s,
        active: r[0] !== 0,
        alive: r[1] !== 0,
        x: r[2] / 65536,
        y: r[3] / 65536,
        angle: r[4],
        hp: r[5],
        frags: r[6],
        deaths: r[7],
        respawnAt: r[8],
        color: r[9],
      });
    }
    return out;
  }

  /** Renders and returns a view of the RGBA frame (valid until the next call). */
  render(slot: number, spectate: boolean): Uint8ClampedArray {
    this.x.render(slot, spectate ? 1 : 0);
    return new Uint8ClampedArray(this.x.memory.buffer, this.x.rgba_ptr(), W * H * 4);
  }

  decodeKillCam(frame: Uint8Array): Uint8ClampedArray | null {
    this.bytes(this.x.killcam_ptr(), frame.length).set(frame);
    if (this.x.decode_killcam(frame.length) !== 0) return null;
    return new Uint8ClampedArray(this.x.memory.buffer, this.x.killcam_rgba_ptr(), W * H * 4).slice();
  }
}
