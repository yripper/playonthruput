// Keyboard and mouse to the engine's button bits (engine/src/game.rs `input`).
// The engine turns in fixed steps, so mouse motion is banked and paid out as
// one turn button per tic.

export const BUTTON = {
  FORWARD: 1,
  BACK: 2,
  STRAFE_LEFT: 4,
  STRAFE_RIGHT: 8,
  TURN_LEFT: 16,
  TURN_RIGHT: 32,
  FIRE: 64,
} as const;

/** Mouse pixels per turn step (one step = 12/1024 of a circle). */
const PIXELS_PER_TURN = 7;
/** Caps banked mouse motion so a flick can't queue a second of turning. */
const MAX_BANKED = PIXELS_PER_TURN * 12;

export class Controls {
  private readonly keys = new Set<string>();
  private mouseFire = false;
  private banked = 0;

  constructor(surface: HTMLElement, onSpectateStep: (dir: number) => void) {
    addEventListener('keydown', (e) => {
      if (e.code === 'BracketLeft') onSpectateStep(-1);
      if (e.code === 'BracketRight') onSpectateStep(1);
      if (this.isGameKey(e.code)) e.preventDefault();
      this.keys.add(e.code);
    });
    addEventListener('keyup', (e) => this.keys.delete(e.code));
    addEventListener('blur', () => {
      this.keys.clear();
      this.mouseFire = false;
    });
    surface.addEventListener('mousedown', () => {
      if (document.pointerLockElement !== surface) surface.requestPointerLock?.();
      this.mouseFire = true;
    });
    addEventListener('mouseup', () => (this.mouseFire = false));
    addEventListener('mousemove', (e) => {
      if (document.pointerLockElement !== surface) return;
      this.banked = Math.max(-MAX_BANKED, Math.min(MAX_BANKED, this.banked + e.movementX));
    });
  }

  private isGameKey(code: string): boolean {
    return ['Space', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(code);
  }

  /** Buttons for one tic; consumes one turn step of banked mouse motion. */
  sample(): number {
    const k = (code: string) => this.keys.has(code);
    let b = 0;
    if (k('KeyW') || k('ArrowUp')) b |= BUTTON.FORWARD;
    if (k('KeyS') || k('ArrowDown')) b |= BUTTON.BACK;
    if (k('KeyA')) b |= BUTTON.STRAFE_LEFT;
    if (k('KeyD')) b |= BUTTON.STRAFE_RIGHT;
    if (k('ArrowLeft') || k('KeyQ')) b |= BUTTON.TURN_LEFT;
    if (k('ArrowRight') || k('KeyE')) b |= BUTTON.TURN_RIGHT;
    if (k('Space') || this.mouseFire) b |= BUTTON.FIRE;
    if (this.banked >= PIXELS_PER_TURN) {
      b |= BUTTON.TURN_RIGHT;
      this.banked -= PIXELS_PER_TURN;
    } else if (this.banked <= -PIXELS_PER_TURN) {
      b |= BUTTON.TURN_LEFT;
      this.banked += PIXELS_PER_TURN;
    }
    return b;
  }
}
