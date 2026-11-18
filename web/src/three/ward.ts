import * as THREE from "three";
import { attackOf, type BodyState, bodyState } from "../record/stats";
import type { HourRecord } from "../record/types";
import { type Cloud, cloudGeometry } from "./anatomy";
import { ease, Flight, type View } from "./camera";
import { inkMaterial, ROLL, ROUTES, reachTime } from "./ink";

/** A bed on the page, in CSS pixels from the canvas's top-left corner. */
export interface Bed {
  x: number;
  y: number;
  length: number;
  height: number;
}

/**
 * A patient on screen, handed between the ward and its page: centre in viewport pixels, length head to feet in CSS
 * pixels, and its roll about the long axis in radians.
 */
export interface OnScreen {
  cx: number;
  cy: number;
  length: number;
  roll: number;
}

const LYING = new THREE.Matrix3().set(0, -1, 0, 1, 0, 0, 0, 0, 1);

/** Half the body's span from hand to hand, in body units (head to feet is 2), with its idle roll. */
const HALF_SPAN = 0.4;

/** Story seconds a whole run takes, its held scar included: past this a patient's ink stands still. */
const STORY = 4.4;

/**
 * The lean toward a pointer at the grid's edge: CSS pixels the patients slide off their beds' centres, and radians. The
 * slide stays inside the few pixels a bed leaves around its patient; the turn is what reads as depth.
 */
const LEAN_SHIFT = 1;
const LEAN_TURN = 0.14;

/**
 * The patient's scale and centre on its bed: as long as 94% of the bed, unless the hands would then reach past its
 * long sides, where they would touch the state pill or the row above. A body shortened to fit keeps its head on the
 * pillow.
 */
export function placeOnBed(b: Bed): { scale: number; x: number } {
  const full = (b.length * 0.94) / 2;
  const scale = Math.min(full, (b.height * 0.49) / HALF_SPAN);
  return { scale, x: b.x - (full - scale) };
}

/**
 * A body's state as its figure draws it. `brain` false: the run never repeated the strain's reference word, so the
 * brain stays unstained while the ink runs past it on the nerves. Left out, the brain inks wherever the chain reaches.
 */
export type Figure = BodyState & { brain?: boolean };

/** How the figure of `body` reads in `hour`: its state, and whether the run reached the brain. */
export const figureState = (hour: HourRecord, body: string): Figure => ({
  ...bodyState(hour, body),
  brain: attackOf(hour, body)?.reached_brain ?? false,
});

/**
 * Instance values for one patient: when its ink stops (story seconds, -1 never), marks (1 for hands a gene held,
 * plus 2 for a brain the run never reached), void, route index.
 */
export function stateVector(s: Figure): [number, number, number, number] {
  const marks = (s.pill === "held" ? 1 : 0) + (s.brain === false ? 2 : 0);
  return [reachTime(s.reach), marks, s.pill === "void" ? 1 : 0, s.route ? ROUTES.indexOf(s.route) : 0];
}

/** A touch screen or a narrow window: a thinner standing body, and a ward that idles at 30 fps. */
export const lean = () => matchMedia("(pointer: coarse)").matches || innerWidth < 700;

/** Twenty patients in one draw call: a shared ~7.5k-point body, instanced with place, state and timing. */
export class Ward {
  readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.OrthographicCamera(-1, 1, 1, -1, -2000, 2000);
  private readonly geometry: THREE.InstancedBufferGeometry;
  private readonly material: THREE.ShaderMaterial;
  private readonly t0 = performance.now();
  private readonly flight = new Flight({ x: 0, y: 0, zoom: 1 });
  private readonly reduced = matchMedia("(prefers-reduced-motion: reduce)");
  private readonly thin = lean();
  private width = 1;
  private height = 1;
  private frameId = 0;
  private frames = 0;
  private visible = true;
  private frozen = false;
  /** Something changed since the last frame drawn. */
  private dirty = true;
  /** Shader seconds until which a sketch, an ink front, a fade or a pose runs. */
  private busy = 0;
  private lastChange = Number.NEGATIVE_INFINITY;
  private fade = { t0: 0, ms: 1 };
  /** One patient's organs warming to umber (from 0 to 1) or back, over `ms` from `t0`. */
  private warmth = { i: -1, from: 0, to: 0, t0: 0, ms: 1 };
  /** Where the pointer is over the stage (-1 to 1 on each axis), and the lean that follows it. */
  private readonly aim = new THREE.Vector2();
  private readonly lean = new THREE.Vector2();
  private lastFrame = performance.now();
  private readonly observer: IntersectionObserver;
  beds: Bed[] = [];

  constructor(
    canvas: HTMLCanvasElement,
    cloud: Cloud,
    private readonly count: number,
  ) {
    this.renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: false,
      alpha: true,
      powerPreference: "high-performance",
    });
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    this.geometry = cloudGeometry(cloud, count);
    this.material = inkMaterial(1, "ward", cloud.heart);
    this.material.uniforms.uOrient.value = LYING;
    const points = new THREE.Points(this.geometry, this.material);
    points.frustumCulled = false;
    this.scene.add(points);
    this.camera.position.z = 1000;
    this.observer = new IntersectionObserver(([entry]) => {
      this.visible = entry?.isIntersecting ?? true;
    });
    this.observer.observe(canvas);
    this.reduced.addEventListener("change", this.onMotion);
    this.onMotion();
    this.loop();
  }

  /** Seconds on the shader clock. */
  now(): number {
    return (performance.now() - this.t0) / 1000;
  }

  /** Reduced motion or a still: every bed holds a fixed angle, and every change lands final at once. */
  private final(): boolean {
    return this.frozen || this.reduced.matches;
  }

  private onMotion = (): void => {
    this.material.uniforms.uMotion.value = this.final() ? 0 : 1;
    this.dirty = true;
  };

  resize(width: number, height: number, beds: Bed[]): void {
    this.width = width;
    this.height = height;
    this.beds = beds;
    this.renderer.setSize(width, height, false);
    const offset = this.geometry.getAttribute("iOffset") as THREE.InstancedBufferAttribute;
    const scale = this.geometry.getAttribute("iScale") as THREE.InstancedBufferAttribute;
    beds.forEach((b, i) => {
      const at = placeOnBed(b);
      offset.setXYZ(i, at.x, -b.y, 0);
      scale.setX(i, at.scale);
    });
    offset.needsUpdate = true;
    scale.needsUpdate = true;
    // Points grow with the body, as they did when a body always spanned 94% of its bed (bed / 160).
    const body = beds.reduce((m, b) => Math.max(m, placeOnBed(b).scale), 0);
    this.material.uniforms.uSize.value = Math.min(1.75, Math.max(0.85, body / 75)) * this.renderer.getPixelRatio();
    this.dirty = true;
  }

  /**
   * Sets every patient's state. A patient whose state changed inks again from `inkIn` seconds from now at `rate`; the
   * rest keep their ink, so an hour change never flickers. A change within 400 ms of the last one (scrubbing, Play)
   * lands final at once.
   */
  setStates(states: Figure[], inkIn: number, rate = 2.4): void {
    const attr = this.geometry.getAttribute("iState") as THREE.InstancedBufferAttribute;
    const time = this.geometry.getAttribute("iTime") as THREE.InstancedBufferAttribute;
    const now = this.now();
    const at = this.final() || now - this.lastChange < 0.4 ? -100 : inkIn;
    this.lastChange = now;
    states.forEach((s, i) => {
      const v = stateVector(s);
      // The attribute holds 32-bit floats: compare in their precision, or every patient counts as changed.
      if (v.every((x, k) => Math.fround(x) === attr.getComponent(i, k))) return;
      attr.setXYZW(i, ...v);
      time.setY(i, now + at);
      time.setZ(i, rate);
    });
    this.busy = Math.max(this.busy, now + at + STORY / rate);
    attr.needsUpdate = true;
    time.needsUpdate = true;
    this.dirty = true;
  }

  /** Per patient, seconds from now its pencil starts, seconds after that its ink starts, and the ink's rate. */
  sketch(delays: number[], inkAfter: number[], rates: number[]): void {
    if (this.final()) {
      this.finish();
      return;
    }
    const time = this.geometry.getAttribute("iTime") as THREE.InstancedBufferAttribute;
    const now = this.now();
    delays.forEach((d, i) => {
      time.setXYZ(i, now + d, now + d + inkAfter[i], rates[i]);
      this.busy = Math.max(this.busy, now + d + Math.max(2, inkAfter[i] + STORY / rates[i]));
    });
    time.needsUpdate = true;
    this.dirty = true;
  }

  /** Ends every sketch, ink run, fade and pose at once. */
  finish(): void {
    const time = this.geometry.getAttribute("iTime") as THREE.InstancedBufferAttribute;
    for (let i = 0; i < this.count; i++) time.setXY(i, -100, -100);
    time.needsUpdate = true;
    const u = this.material.uniforms;
    u.uSolo.value = -1;
    u.uSoloFade.value = 0;
    u.uPose.value.set(-1, -100, 0);
    this.warmth = { i: -1, from: 0, to: 0, t0: 0, ms: 1 };
    this.busy = 0;
    this.dirty = true;
  }

  /** A picture, not a scene: every patient finished, at a fixed angle, and nothing moves. */
  still(): void {
    this.frozen = true;
    this.onMotion();
    this.finish();
  }

  /** Fades every patient but `i` out over `ms`. */
  solo(i: number, ms: number): void {
    if (this.final()) return;
    this.material.uniforms.uSolo.value = i;
    this.fade = { t0: this.now(), ms };
    this.busy = Math.max(this.busy, this.now() + ms / 1000);
  }

  /**
   * Patient `i`'s organs warm from umber share `from` to `to` over `ms`: to its page's umber as it flies there, back to
   * the ward's graphite as it returns.
   */
  warm(i: number, from: number, to: number, ms: number): void {
    const t0 = this.final() ? this.now() - ms / 1000 : this.now();
    this.warmth = { i, from, to, t0, ms: Math.max(1, ms) };
    this.busy = Math.max(this.busy, this.now() + ms / 1000);
    this.dirty = true;
  }

  /** Patient `i` stands at roll `roll`, as its page left it, and lies down on its bed over 0.6 s. */
  pose(i: number, roll: number): void {
    this.material.uniforms.uPose.value.set(i, this.now(), roll);
    this.busy = Math.max(this.busy, this.now() + 0.6);
    this.dirty = true;
  }

  /** Patient `i`'s roll `ahead` seconds from now, as the shader turns it. */
  rollAt(i: number, ahead: number): number {
    const off = this.geometry.getAttribute("iOffset") as THREE.InstancedBufferAttribute;
    const phase = (this.now() + ahead) * this.material.uniforms.uMotion.value * 0.35;
    return ROLL * Math.sin(phase + off.getX(i) * 0.013 + off.getY(i) * 0.021);
  }

  /** The view centred on the canvas at zoom 1, or framed on one bed. */
  homeView(): View {
    return { x: this.width / 2, y: this.height / 2, zoom: 1 };
  }

  /**
   * A view with bed `i` large and its patient centred in the visible part of the canvas (`visible` px tall, from
   * `top` px below the canvas's top): a patient shortened to fit its bed keeps its head on the pillow, so it sits left
   * of the bed's centre.
   */
  closeUp(i: number, visible: number, share: number, top = 0): View {
    const b = this.beds[i];
    const zoom = Math.max(1, (share * Math.min(this.width, visible * 2.4)) / b.length);
    return { x: placeOnBed(b).x, y: b.y + (this.height / 2 - top - visible / 2) / zoom, zoom };
  }

  /** The view that puts patient `i` where a page left it standing: `f` on screen, `rect` the canvas's box. */
  standView(i: number, f: OnScreen, rect: DOMRect): View {
    const b = this.beds[i];
    const at = placeOnBed(b);
    const zoom = f.length / (2 * at.scale);
    return {
      x: at.x - (f.cx - rect.left - this.width / 2) / zoom,
      y: b.y - (f.cy - rect.top - this.height / 2) / zoom,
      zoom,
    };
  }

  /** Leans every patient toward a pointer at (`x`, `y`), each -1 to 1 across the stage; (0, 0) lays them straight. */
  look(x: number, y: number): void {
    this.aim.set(Math.max(-1, Math.min(1, x)), Math.max(-1, Math.min(1, y)));
  }

  fly(to: View, ms: number): void {
    if (ms <= 0) this.flight.jump(to);
    else this.flight.go(to, ms, performance.now());
    this.dirty = true;
  }

  private loop = (): void => {
    this.frameId = requestAnimationFrame(this.loop);
    if (!this.visible) return;
    const t = performance.now();
    const now = this.now();
    const u = this.material.uniforms;
    const moving = !this.flight.done(t) || now < this.busy;
    // Nothing runs: a still or reduced motion draws on a change only, a phone every other frame (the roll's 18 s
    // period hides it).
    if (!moving && !this.dirty && (u.uMotion.value === 0 || (this.thin && this.frames++ % 2 === 1))) return;
    this.dirty = false;
    // The lean settles on the pointer in about half a second; a still or reduced motion never leans.
    const dt = Math.min(0.1, (t - this.lastFrame) / 1000);
    this.lastFrame = t;
    if (this.final()) this.aim.set(0, 0);
    this.lean.lerp(this.aim, this.final() ? 1 : 1 - Math.exp(-6 * dt));
    const v = this.flight.at(t);
    const c = this.camera;
    c.left = -this.width / 2;
    c.right = this.width / 2;
    c.top = this.height / 2;
    c.bottom = -this.height / 2;
    c.zoom = v.zoom;
    // A pointer to the right steps the camera right: the patients, lying above their beds, slide a few pixels left of
    // them and turn as if seen from there.
    c.position.set(v.x + (LEAN_SHIFT * this.lean.x) / v.zoom, -v.y - (LEAN_SHIFT * this.lean.y) / v.zoom, 1000);
    u.uLook.value.set(-LEAN_TURN * this.lean.x, -LEAN_TURN * this.lean.y);
    c.updateProjectionMatrix();
    u.uTime.value = now;
    u.uZoom.value = v.zoom;
    u.uLens.value = v.zoom;
    const w = this.warmth;
    u.uTint.value = w.from + (w.to - w.from) * ease(Math.min(1, Math.max(0, (now - w.t0) / (w.ms / 1000))));
    u.uTintWho.value = w.i;
    if (u.uSolo.value >= 0) u.uSoloFade.value = ease(Math.min(1, (now - this.fade.t0) / (this.fade.ms / 1000)));
    this.renderer.render(this.scene, c);
  };

  dispose(): void {
    cancelAnimationFrame(this.frameId);
    this.observer.disconnect();
    this.reduced.removeEventListener("change", this.onMotion);
    this.geometry.dispose();
    this.material.dispose();
    this.renderer.dispose();
  }
}
