import * as THREE from "three";
import { type Anatomy, buildCloud, type Cloud, cloudGeometry, ORGAN_ANCHORS } from "./anatomy";
import { ease, Flight, type View } from "./camera";
import { COLOURS, CONTEXT, chainAt, inkMaterial, knotTime, PART, type Part, PENCIL, ROLL, reachTime } from "./ink";
import { type Figure, lean, type OnScreen, stateVector } from "./ward";

export type Organ = "eyes" | "ears" | "brain" | "hands" | "heart";
export const ORGANS: Organ[] = ["eyes", "ears", "brain", "hands", "heart"];
/** The labels' order down the page: the ear sits right of the eye, so its label goes above and no leaders cross. */
const LABEL_ORDER: Organ[] = ["brain", "ears", "eyes", "heart", "hands"];

/**
 * How the figure inks organ `o` for `s`, so its label can say the same: "ink" (eosin) where the ink reaches, "held"
 * for hands a gene stopped (they scar violet), "" where no ink lands. A run that never reached the brain inks it no
 * further than the way in, which only a memory strain takes.
 */
export function organTone(s: Figure, o: Organ): "" | "ink" | "held" {
  if (!s.route) return "";
  const reach = o === "brain" && s.brain === false ? Math.min(s.reach, 1) : s.reach;
  const start = chainAt(s.route, PART[o], 0);
  if (start < 0 || start >= reach) return "";
  return o === "hands" && s.pill === "held" ? "held" : "ink";
}

let full: Cloud | null = null;
let building: Promise<Cloud> | null = null;

let thin: number | null = null;
/**
 * Points a phone draws around the organs: three quarters of the skin, bones, muscle, viscera and arteries (the
 * material darkens them to keep the tone). Fixed for the page load, like the one cloud built with it.
 */
const density = () => {
  thin ??= lean() ? 0.75 : 1;
  return thin;
};

let built: (c: Cloud) => void = () => {};
/** The standing body once some view has built it: a view that waits takes it from here instead of starting it. */
const standing = new Promise<Cloud>((resolve) => {
  built = resolve;
});

/** Builds the standing body once per page load, in a worker, so neither a fly-in nor the ward's frames wait on it. */
export function warmBody(anatomy: Anatomy): Promise<Cloud> {
  building ??= new Promise<Cloud>((resolve) => {
    const d = density();
    const keep = (c: Cloud) => {
      full ??= c;
      built(full);
      resolve(full);
    };
    try {
      const worker = new Worker(new URL("./cloud.worker.ts", import.meta.url), { type: "module" });
      worker.onmessage = (e: MessageEvent<Cloud>) => {
        worker.terminate();
        keep(e.data);
      };
      worker.onerror = () => {
        worker.terminate();
        keep(buildCloud(anatomy, true, d));
      };
      worker.postMessage({ anatomy, density: d });
    } catch {
      keep(buildCloud(anatomy, true, d));
    }
  });
  return building;
}

/** Organ framing: the height of body (in units) a close-up holds. */
const SPAN: Record<Organ, number> = { eyes: 0.42, ears: 0.42, brain: 0.5, hands: 0.7, heart: 0.6 };
/**
 * A close-up's turn: the eyes a little to the side so the ears show their curve, the ear in profile, the brain with its
 * lobes and the ear, the palm with depth, the apex forward.
 */
const YAW: Record<Organ, number> = { eyes: 0.35, ears: 1.05, brain: 0.75, hands: 0.25, heart: 0.35 };
/** Idle roll in a close-up: the organ stays alive without swinging away. */
const CLOSE_ROLL = 0.08;
/** Organs framed on the head's midline: the ear sits at the head's edge, and centring there cuts the face in two. */
const HEAD: Organ[] = ["eyes", "ears", "brain"];

/** Per organ: whether the pencil has drawn it yet, and whether the ink has reached it (so its label may say so). */
export interface Phase {
  drawn: boolean;
  inked: boolean;
}

/** Story seconds after the ink starts when it reaches organ `o` of `s`; held hands count once their scar has formed. */
function inkedAt(s: Figure, o: Organ): number {
  if (!organTone(s, o) || !s.route) return Number.POSITIVE_INFINITY;
  return knotTime(chainAt(s.route, PART[o], 0)) + (o === "hands" && s.pill === "held" ? 0.45 : 0);
}

/** `p` turned about the body's long axis by `angle`, as the shader turns every point. */
export function turn(p: THREE.Vector3, angle: number, out = new THREE.Vector3()): THREE.Vector3 {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  return out.set(p.x * c - p.z * s, p.y, p.x * s + p.z * c);
}

/** Where the label column starts: `gap` right of the right-most anchor in view, at most `room` from the right edge. */
export function labelX(
  at: Record<Organ, [number, number]>,
  width: number,
  height: number,
  gap: number,
  room: number,
): number {
  const seen = ORGANS.map((o) => at[o]).filter(([x, y]) => x >= 0 && x <= width && y >= 0 && y <= height);
  return Math.min(Math.max(width / 2, ...seen.map(([x]) => x)) + gap, width - room);
}

/**
 * Label heights for the anchors `at`, down the page in a fixed order: each at its anchor's height or `gap` below the
 * label above, whichever is lower. Sorting by the live heights instead lets the eye and the ear, at about one height,
 * swap labels from frame to frame as the figure rolls, and their leaders cross.
 */
export function spread(at: Record<Organ, [number, number]>, gap: number): Record<Organ, number> {
  const out = {} as Record<Organ, number>;
  let last = Number.NEGATIVE_INFINITY;
  for (const o of LABEL_ORDER) {
    last = Math.max(at[o][1], last + gap);
    out[o] = last;
  }
  return out;
}

/** Every organ drawn and inked: the phase of a figure that is already finished. */
export const FINISHED = Object.fromEntries(ORGANS.map((o) => [o, { drawn: true, inked: true }])) as Record<
  Organ,
  Phase
>;

const smooth = (a: number, b: number, x: number) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};

/**
 * The standing figure as a still on a 2D canvas, for a browser that cannot draw WebGL: the points of `c` as `state`
 * ends, front on, framed as the live view frames the whole body; `shift` moves it right in body units, as a panel on
 * the left does. `only` inks just those parts, as the check-up does. Returns where the organs of `anchors` land, in
 * CSS pixels from the canvas's corner, for the labels; null when the canvas cannot draw.
 */
export function drawStill(
  canvas: HTMLCanvasElement,
  c: Cloud,
  anchors: Record<Organ, readonly number[]>,
  state: Figure,
  opts: { only?: readonly number[]; shift?: number } = {},
): Record<Organ, [number, number]> | null {
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  const g = canvas.getContext("2d");
  if (!g || w < 1 || h < 1) return null;
  const dpr = Math.min(devicePixelRatio, 2);
  canvas.width = Math.round(w * dpr);
  canvas.height = Math.round(h * dpr);
  // The live view at zoom 1 holds 2.24 body units of height (head to feet is 2), or 1.24 of width when narrow.
  const px = Math.min(h / 2.24, w / 1.24);
  const x0 = w / 2 + (opts.shift ?? 0) * px;
  const y0 = h / 2 + 0.02 * px;
  const [reach, marks, isVoid, route] = stateVector(state);
  const thin = density();
  // A square of the point sprite's area, at the live view's point size.
  const side = (w < 700 ? 1.1 : 1.35) * dpr * 0.89;
  // As the shader draws it: pencil by part, umber organs with a contour, eosin ink, violet hands a gene held.
  const layers: Record<string, number[]> = {
    [COLOURS.graphite]: [],
    [COLOURS.umber]: [],
    [COLOURS.steel]: [],
    [COLOURS.eosin]: [],
    [COLOURS.violet]: [],
  };
  for (let i = 0; i < c.count; i++) {
    const part = c.part[i];
    const organ = part >= PART.eyes && part <= PART.heart;
    const depth = smooth(-0.12, 0.1, c.position[i * 3 + 2]);
    const [nx, ny, nz] = [c.nrm[i * 3], c.nrm[i * 3 + 1], c.nrm[i * 3 + 2]];
    const rim = 0.5 + 1.1 * (nx * nx + ny * ny + nz * nz > 0.5 ? (1 - Math.abs(nz)) ** 1.5 : 0.5);
    const chain = c.chain.subarray(i * 4, i * 4 + 4);
    const arrive = opts.only ? (opts.only.includes(part) ? (chain.find((x) => x >= 0) ?? -1) : -1) : chain[route];
    const stop = part === PART.brain && marks >= 2 ? Math.min(reach, reachTime(1)) : reach;
    let colour: string;
    let alpha: number;
    // Nerves draw a little larger, and ink swells a point: most along a nerve or a hand, least on the eyes and ears.
    let size = side * (part > PART.artery ? 1.15 : 1);
    if (!isVoid && arrive >= 0 && arrive <= stop + 0.0001) {
      colour = part === PART.hands && (marks === 1 || marks === 3) ? COLOURS.violet : COLOURS.eosin;
      alpha = part === PART.artery ? 0.5 : organ ? Math.min(1, 0.85 * (0.5 + 0.5 * depth) * rim) : 0.92;
      const quiet = part === PART.eyes || part === PART.ears || part === PART.artery;
      size *= quiet ? 1.1 : part === PART.brain || part === PART.heart ? 1.2 : 1.45;
    } else {
      colour = isVoid ? COLOURS.steel : organ ? COLOURS.umber : COLOURS.graphite;
      alpha = Math.min(1, PENCIL[part] / (CONTEXT.includes(part as Part) ? thin : 1));
      alpha *= (organ ? (0.5 + 0.5 * depth) * rim : 0.8 + 0.2 * depth) * (isVoid ? 0.75 : 1);
    }
    layers[colour].push((x0 + c.position[i * 3] * px) * dpr, (y0 - c.position[i * 3 + 1] * px) * dpr, alpha, size);
  }
  g.clearRect(0, 0, canvas.width, canvas.height);
  for (const [colour, list] of Object.entries(layers)) {
    g.fillStyle = colour;
    for (let k = 0; k < list.length; k += 4) {
      const s = list[k + 3];
      g.globalAlpha = Math.min(1, list[k + 2]);
      g.fillRect(list[k] - s / 2, list[k + 1] - s / 2, s, s);
    }
  }
  g.globalAlpha = 1;
  return Object.fromEntries(ORGANS.map((o) => [o, [x0 + anchors[o][0] * px, y0 - anchors[o][1] * px]])) as Record<
    Organ,
    [number, number]
  >;
}

/**
 * One patient at full density, standing, with its organs projected to the page for labels (`onProject`, every frame
 * drawn). `still`: a picture, final on its first frame, that never moves.
 */
export class BodyView {
  private readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(24, 1, 0.1, 50);
  private readonly material: THREE.ShaderMaterial;
  private readonly anchors: Record<Organ, THREE.Vector3>;
  private readonly flight = new Flight({ x: 0, y: 0.02, zoom: 1, yaw: 0, amp: ROLL, focus: 0 });
  private readonly t0 = performance.now();
  private readonly reduced = matchMedia("(prefers-reduced-motion: reduce)");
  private readonly observer: IntersectionObserver;
  private readonly q = new THREE.Vector3();
  private cloud: Cloud | null = null;
  private geometry: THREE.InstancedBufferGeometry | null = null;
  private width = 1;
  private height = 1;
  /** The canvas's top-left corner on the page, read on resize: a page that is being left has no layout to read. */
  private corner = [0, 0];
  private frameId = 0;
  private visible = true;
  private disposed = false;
  /** Something changed since the last frame drawn. */
  private dirty = true;
  /** Shader seconds until which a sketch or an ink front runs. */
  private busy = 0;
  private shown: { state: Figure; sketchAt: number; inkAt: number } | null = null;
  private only: readonly number[] | null = null;
  /** A fly-in: the patient's start, lying as the ward left it, in body units around this view. */
  private lift: { t0: number; ms: number; scale: number; x: number; y: number } | null = null;
  /** A replay: when the ward's ink starts to lift, in shader seconds. */
  private drainAt: number | null = null;
  /** Shader seconds when the pencil and the ink of the shown state started. */
  private clock = { sketch: -100, ink: -100 };
  /** Settles once the figure can draw: at once when the cloud was built, else when the worker hands it over. */
  readonly ready: Promise<void>;

  constructor(
    canvas: HTMLCanvasElement,
    anatomy: Anatomy,
    private readonly onProject: (at: Record<Organ, [number, number]>, phase: Record<Organ, Phase>) => void,
    private readonly opts: { still?: boolean; wait?: boolean } = {},
  ) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: false, alpha: true });
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    this.material = inkMaterial(1, "body", anatomy.groups.heart.centre, density());
    const a = ORGAN_ANCHORS(anatomy);
    this.anchors = Object.fromEntries(ORGANS.map((o) => [o, new THREE.Vector3(...a[o])])) as Record<
      Organ,
      THREE.Vector3
    >;
    this.observer = new IntersectionObserver(([entry]) => {
      this.visible = entry?.isIntersecting ?? true;
    });
    this.observer.observe(canvas);
    this.reduced.addEventListener("change", this.onMotion);
    this.onMotion();
    // A still must be whole on its first frame; a live figure takes the worker's cloud once it is there, and one that
    // can `wait` takes it once the ward has built it after its opening, instead of building it now.
    if (!full && opts.still) {
      full = buildCloud(anatomy, true, density());
      built(full);
    }
    if (full) {
      this.attach(full);
      this.ready = Promise.resolve();
    } else {
      this.ready = (opts.wait ? standing : warmBody(anatomy)).then((c) => {
        if (!this.disposed) this.attach(c);
      });
    }
    this.loop();
  }

  now(): number {
    return (performance.now() - this.t0) / 1000;
  }

  /** A still, or reduced motion: no roll, beat or shimmer, and every state lands final at once. */
  private final(): boolean {
    return Boolean(this.opts.still) || this.reduced.matches;
  }

  private onMotion = (): void => {
    this.material.uniforms.uMotion.value = this.final() ? 0 : 1;
    this.dirty = true;
  };

  private attach(c: Cloud): void {
    this.cloud = c;
    const g = cloudGeometry(c, 1);
    (g.getAttribute("iScale") as THREE.InstancedBufferAttribute).setX(0, 1);
    // Nothing inked until a state is shown.
    (g.getAttribute("iState") as THREE.InstancedBufferAttribute).setX(0, -1);
    const points = new THREE.Points(g, this.material);
    points.frustumCulled = false;
    this.scene.add(points);
    this.geometry = g;
    if (this.only) this.inkOnly(this.only);
    this.apply();
    // Off screen, one draw of what is shown puts the points and the shaders on the GPU, so the first frame in view
    // costs no more than the next.
    if (!this.visible) this.renderer.render(this.scene, this.camera);
  }

  resize(width: number, height: number): void {
    // A pane with no size yet (a sheet still opening, a section not laid out) keeps the last size: no NaN projection.
    if (width < 1 || height < 1) return;
    this.width = width;
    this.height = height;
    this.renderer.setSize(width, height, false);
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.material.uniforms.uSize.value = (width < 700 ? 1.1 : 1.35) * this.renderer.getPixelRatio();
    const r = this.renderer.domElement.getBoundingClientRect();
    this.corner = [r.left + scrollX, r.top + scrollY];
    // A resize clears the canvas after this frame's draw: draw again now, or a resizing pane flashes blank.
    if (this.geometry) this.renderer.render(this.scene, this.camera);
    this.dirty = true;
  }

  /** Draws the body from the head down, then runs the ink of `state` from `inkAt` seconds (-100: both done). */
  show(state: Figure, sketchAt: number, inkAt: number): void {
    this.drainAt = null;
    this.material.uniforms.uDrain.value = 0;
    const final = this.final();
    this.shown = { state, sketchAt: final ? -100 : sketchAt, inkAt: final ? -100 : inkAt };
    this.apply();
  }

  /** Writes the shown state into the patient, its times counted from now: a cloud that arrives late starts fresh. */
  private apply(): void {
    const g = this.geometry;
    const s = this.shown;
    if (!g || !s) return;
    const now = this.now();
    const state = g.getAttribute("iState") as THREE.InstancedBufferAttribute;
    const time = g.getAttribute("iTime") as THREE.InstancedBufferAttribute;
    state.setXYZW(0, ...stateVector(s.state));
    time.setXYZ(0, now + s.sketchAt, now + s.inkAt, 1);
    this.clock = { sketch: now + s.sketchAt, ink: now + s.inkAt };
    state.needsUpdate = true;
    time.needsUpdate = true;
    this.busy = Math.max(this.busy, now + Math.max(s.sketchAt + 2.5, s.inkAt + 4.4));
    this.dirty = true;
  }

  /**
   * Shows `state` inked as the ward left it; at `at` seconds the ink lifts (350 ms), and 0.45 s later the venom runs
   * again from the way in at rate 1.
   */
  replay(state: Figure, at: number): void {
    this.show(state, -100, -100);
    if (this.final()) return;
    this.drainAt = this.now() + at;
    this.busy = Math.max(this.busy, this.drainAt + 0.45 + 4.4);
  }

  /** Inks only `parts`, each where the first route that reaches it puts it: the check-up inks organs with lesions. */
  inkOnly(parts: readonly number[]): void {
    this.only = parts;
    const c = this.cloud;
    if (!c || !this.geometry) return;
    const only = new Float32Array(c.chain.length).fill(-1);
    for (let i = 0; i < c.count; i++) {
      if (parts.includes(c.part[i]))
        only.fill(c.chain.subarray(i * 4, i * 4 + 4).find((x) => x >= 0) ?? -1, i * 4, i * 4 + 4);
    }
    this.geometry.setAttribute("chain", new THREE.BufferAttribute(only, 4));
    this.dirty = true;
  }

  /**
   * Whole body, or one organ turned to face you; the body sits right of centre when a panel lies over the stage's left
   * (`panel`, as the page lays it out), and an organ sits left of centre on a portrait screen, whose labels take the
   * right edge.
   */
  focus(organ: Organ | null, panel: boolean, ms: number): void {
    const yaw = organ ? YAW[organ] : 0;
    const target: View = organ
      ? {
          x: HEAD.includes(organ) ? 0 : turn(this.anchors[organ], yaw, this.q).x,
          y: this.anchors[organ].y,
          zoom: 2.2 / SPAN[organ],
          yaw,
          amp: CLOSE_ROLL,
          focus: 1,
        }
      : { x: 0, y: 0.02, zoom: 1, yaw: 0, amp: ROLL, focus: 0 };
    // The organ takes its weight at once and the focus eases in with the flight; leaving, the focus eases out on it.
    if (organ) this.material.uniforms.uFocusPart.value = PART[organ];
    const shift = panel ? -0.22 : organ && this.width < this.height ? 0.12 : 0;
    const to = { ...target, x: target.x + (shift * 2.2) / target.zoom };
    if (ms <= 0 || this.final()) this.flight.jump(to);
    else this.flight.go(to, ms, performance.now());
    this.dirty = true;
  }

  /** The patient arrives as the ward left it (`from`: lying, inked, rolled) and stands up into this view over `ms`. */
  rise(from: OnScreen, ms: number): void {
    if (this.final()) return;
    const v = this.flight.at(performance.now());
    const px = this.px(v.zoom);
    this.lift = {
      t0: performance.now(),
      ms,
      scale: from.length / (2 * px),
      x: v.x + (from.cx - (this.corner[0] - scrollX) - this.width / 2) / px,
      y: v.y - (from.cy - (this.corner[1] - scrollY) - this.height / 2) / px,
    };
    // The idle roll goes on from the angle the patient had on its bed.
    const u = this.material.uniforms;
    u.uRollPhase.value = Math.asin(Math.min(1, Math.max(-1, from.roll / ROLL))) - 0.35 * this.now();
    this.dirty = true;
  }

  /** Where the standing patient is on screen, for the ward to lay it down; null in a close-up or while it rises. */
  figure(): OnScreen | null {
    const v = this.flight.at(performance.now());
    if (v.zoom > 1.2 || this.lift) return null;
    const px = this.px(v.zoom);
    return {
      cx: this.corner[0] - scrollX + this.width / 2 - v.x * px,
      cy: this.corner[1] - scrollY + this.height / 2 + v.y * px,
      length: 2 * px,
      roll: this.angle(v),
    };
  }

  private distance(zoom: number): number {
    const half = Math.tan(THREE.MathUtils.degToRad(this.camera.fov / 2));
    return Math.max(1.12 / half, 0.62 / (half * this.camera.aspect)) / zoom;
  }

  /** CSS pixels per body unit at depth 0. */
  private px(zoom: number): number {
    return this.height / (2 * this.distance(zoom) * Math.tan(THREE.MathUtils.degToRad(this.camera.fov / 2)));
  }

  /** The figure's turn about its long axis: a close-up's yaw plus the idle roll, as the shader computes it. */
  private angle(v: View): number {
    const u = this.material.uniforms;
    return (v.yaw ?? 0) + (v.amp ?? ROLL) * Math.sin(this.now() * u.uMotion.value * 0.35 + u.uRollPhase.value);
  }

  private rising(t: number): void {
    const r = this.lift;
    if (!r) return;
    const k = ease(Math.min(1, (t - r.t0) / r.ms));
    const s = r.scale + (1 - r.scale) * k;
    const a = (Math.PI / 2) * (1 - k);
    const m = this.material.uniforms.uOrient.value as THREE.Matrix3;
    m.set(Math.cos(a) * s, -Math.sin(a) * s, 0, Math.sin(a) * s, Math.cos(a) * s, 0, 0, 0, s);
    const off = this.geometry?.getAttribute("iOffset") as THREE.InstancedBufferAttribute | undefined;
    if (off) {
      off.setXYZ(0, r.x * (1 - k), r.y * (1 - k), 0);
      off.needsUpdate = true;
    }
    if (k >= 1) this.lift = null;
  }

  private draining(g: THREE.InstancedBufferGeometry, now: number): void {
    const d = this.drainAt ?? now;
    const u = this.material.uniforms;
    u.uDrain.value = ease(Math.min(1, Math.max(0, (now - d) / 0.35)));
    if (now < d + 0.45) return;
    u.uDrain.value = 0;
    const time = g.getAttribute("iTime") as THREE.InstancedBufferAttribute;
    time.setY(0, d + 0.45);
    time.needsUpdate = true;
    this.clock.ink = d + 0.45;
    this.drainAt = null;
  }

  private loop = (): void => {
    this.frameId = requestAnimationFrame(this.loop);
    if (!this.visible) return;
    const t = performance.now();
    const now = this.now();
    const u = this.material.uniforms;
    const moving = !this.flight.done(t) || now < this.busy || this.lift !== null || this.drainAt !== null;
    // A still, or reduced motion, once the figure stands finished: draw again on a change only.
    if (u.uMotion.value === 0 && !moving && !this.dirty) return;
    this.dirty = false;
    this.rising(t);
    if (this.geometry && this.drainAt !== null) this.draining(this.geometry, now);
    const v = this.flight.at(t);
    this.camera.position.set(v.x, v.y, this.distance(v.zoom));
    this.camera.lookAt(v.x, v.y, 0);
    u.uTime.value = now;
    u.uYaw.value = v.yaw ?? 0;
    u.uRoll.value = v.amp ?? ROLL;
    u.uFocus.value = v.focus ?? 0;
    u.uLens.value = v.zoom;
    this.renderer.render(this.scene, this.camera);
    const angle = this.angle(v);
    const at = {} as Record<Organ, [number, number]>;
    const phase = {} as Record<Organ, Phase>;
    // The pencil reaches a point at (order / 0.75) s, order running head to feet (anatomy.ts); a label waits for the
    // middle of its organ's spread. A drained replay holds every label at rest until the ink runs again.
    const sketched = (now - this.clock.sketch) * 0.75;
    const inking = this.drainAt !== null && now >= this.drainAt ? Number.NEGATIVE_INFINITY : now - this.clock.ink;
    for (const o of ORGANS) {
      turn(this.anchors[o], angle, this.q).project(this.camera);
      at[o] = [((this.q.x + 1) / 2) * this.width, ((1 - this.q.y) / 2) * this.height];
      const s = this.shown?.state;
      phase[o] = {
        drawn: Boolean(this.geometry) && sketched >= 0.72 * (1 - (this.anchors[o].y + 1) / 2) + 0.14,
        inked: Boolean(this.geometry && s) && inking >= (s ? inkedAt(s, o) : 0),
      };
    }
    this.onProject(at, phase);
  };

  dispose(): void {
    this.disposed = true;
    cancelAnimationFrame(this.frameId);
    this.observer.disconnect();
    this.reduced.removeEventListener("change", this.onMotion);
    this.geometry?.dispose();
    this.material.dispose();
    this.renderer.dispose();
  }
}
