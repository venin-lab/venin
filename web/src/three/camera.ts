export const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2);

/** A camera, and for the standing body its turn: fixed yaw, idle roll amplitude and organ focus (0 to 1). */
export interface View {
  x: number;
  y: number;
  zoom: number;
  yaw?: number;
  amp?: number;
  focus?: number;
}

/** A camera move between two views over `ms`, eased at both ends. */
export class Flight {
  private from: View;
  private to: View;
  private t0 = 0;
  private ms = 0;

  constructor(view: View) {
    this.from = { ...view };
    this.to = { ...view };
  }

  go(to: View, ms: number, now: number): void {
    this.from = this.at(now);
    this.to = { ...to };
    this.t0 = now;
    this.ms = ms;
  }

  jump(to: View): void {
    this.from = { ...to };
    this.to = { ...to };
    this.ms = 0;
  }

  done(now: number): boolean {
    return this.ms === 0 || now >= this.t0 + this.ms;
  }

  at(now: number): View {
    if (this.done(now)) return { ...this.to };
    const k = ease((now - this.t0) / this.ms);
    const out: View = { ...this.to };
    for (const key of Object.keys(this.to) as (keyof View)[]) {
      const b = this.to[key] as number;
      const a = this.from[key] ?? b;
      // Zoom is interpolated in log space so a pull-back feels even from start to end.
      out[key] = key === "zoom" ? Math.exp(Math.log(a) + (Math.log(b) - Math.log(a)) * k) : a + (b - a) * k;
    }
    return out;
  }
}

/** The view that frames a box of `w` x `h` around (x, y) inside a viewport, with a margin share. */
export function frame(x: number, y: number, w: number, h: number, vw: number, vh: number, margin = 0.15): View {
  return { x, y, zoom: Math.min(vw / (w * (1 + margin)), vh / (h * (1 + margin))) };
}
