import * as THREE from "three";
import { arrival, CONTEXT, PART, type Part, ROUTES } from "./ink";

type V = [number, number, number];

interface Group {
  start: number;
  count: number;
  centre: V;
}

export interface Anatomy {
  unit: number;
  credit: string;
  groups: Record<string, Group>;
  paths: Record<string, V[]>;
  raw: Int16Array;
}

export interface Cloud {
  count: number;
  position: Float32Array;
  part: Float32Array;
  u: Float32Array;
  order: Float32Array;
  /** Per point and route (ROUTES order): story seconds after the ink starts when the front reaches it, -1 never. */
  chain: Float32Array;
  /** Contour normals of the brain, the heart and the eyeballs; zero for parts drawn without a contour. */
  nrm: Float32Array;
  /** The heartbeat's pivot. */
  heart: V;
}

let cached: Promise<Anatomy> | null = null;

/** BodyParts3D point clouds (int16 xyz times `unit`), sampled by the prototype's anatomy.py. */
export function loadAnatomy(): Promise<Anatomy> {
  cached ??= Promise.all([
    fetch("/anatomy/anatomy.json").then((r) => r.json()),
    fetch("/anatomy/anatomy.bin").then((r) => r.arrayBuffer()),
  ]).then(([meta, bin]) => ({ ...meta, raw: new Int16Array(bin) }) as Anatomy);
  return cached;
}

export function rng(seed: number): () => number {
  let s = seed | 0;
  return () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function points(a: Anatomy, name: string): V[] {
  const g = a.groups[name];
  if (!g) return [];
  const out: V[] = [];
  for (let n = g.start; n < g.start + g.count; n++) {
    out.push([a.raw[n * 3] * a.unit, a.raw[n * 3 + 1] * a.unit, a.raw[n * 3 + 2] * a.unit]);
  }
  return out;
}

/** anatomy.py puts isa("external ear") into the eyes group: the auricles are its points beyond |x| 0.058. */
const isAuricle = (p: V) => Math.abs(p[0]) >= 0.058;

/** The eyeballs, and the auricles from their own "ears" group (split off the eyes group in data that has none). */
export function eyesAndEars(a: Anatomy): { eyes: V[]; ears: V[] } {
  const eyes = points(a, "eyes");
  return { eyes: eyes.filter((p) => !isAuricle(p)), ears: a.groups.ears ? points(a, "ears") : eyes.filter(isAuricle) };
}

/** The aortic root above the heart, drawn and inked with it so the heart reads as a heart and not a blob. */
const aorticRoot =
  (c: V) =>
  ([x, y, z]: V) => {
    const [dx, dy, dz] = [x - c[0], y - c[1], z - c[2]];
    return dy > 0.035 && dy < 0.125 && dx > -0.055 && dx < 0.035 && dz > -0.06 && dz < 0.04;
  };

/** Contours: the brain and the heart as ellipsoids, each eyeball as a sphere (centre, radii; fitted to anatomy.bin). */
const BRAIN: [V, V] = [
  [0, 0.891, -0.037],
  [0.08, 0.096, 0.104],
];
const HEART: [V, V] = [
  [0.0255, 0.519, 0.0134],
  [0.067, 0.06, 0.0615],
];
const EYE: V = [0.035, 0.8565, 0.048];
const EYE_RADIUS = 0.0152;

function normalOf(part: Part, [x, y, z]: V): V {
  const along = ([c, r]: [V, V]): V => {
    const n: V = [(x - c[0]) / (r[0] * r[0]), (y - c[1]) / (r[1] * r[1]), (z - c[2]) / (r[2] * r[2])];
    const l = Math.hypot(...n) || 1;
    return [n[0] / l, n[1] / l, n[2] / l];
  };
  if (part === PART.brain) return along(BRAIN);
  if (part === PART.heart) return along(HEART);
  if (part === PART.eyes)
    return along([
      [Math.sign(x) * EYE[0], EYE[1], EYE[2]],
      [1, 1, 1],
    ]);
  return [0, 0, 0];
}

function take<T>(list: T[], count: number, rnd: () => number): T[] {
  if (count >= list.length) return list;
  const copy = [...list];
  for (let i = copy.length - 1; i > 0; i--) {
    const j = Math.floor(rnd() * (i + 1));
    [copy[i], copy[j]] = [copy[j], copy[i]];
  }
  return copy.slice(0, count);
}

function mean(list: V[]): V {
  const s = list.reduce<V>((acc, p) => [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]], [0, 0, 0]);
  return [s[0] / list.length, s[1] / list.length, s[2] / list.length];
}

interface Spec {
  part: Part;
  pts: V[];
  /** Ink spreads outward from the nearest of these. */
  from: V[];
}

/** Points per part for one ward patient (about 6.4k plus its nerves); the body page takes every sampled point. */
const WARD_BUDGET: Partial<Record<Part, number>> = {
  [PART.skin]: 2000,
  [PART.bones]: 1300,
  [PART.muscle]: 450,
  [PART.organs]: 420,
  [PART.eyes]: 160,
  [PART.ears]: 150,
  [PART.brain]: 700,
  [PART.hands]: 460,
  [PART.heart]: 380,
  [PART.artery]: 420,
};

/** Organ anchors; the eye, ear and hand on the labels' side (x > 0), so no leader crosses the face. */
export const ORGAN_ANCHORS = (a: Anatomy) => {
  const { eyes, ears } = eyesAndEars(a);
  return {
    eyes: mean(eyes.filter((p) => p[0] > 0)),
    ears: mean(ears.filter((p) => p[0] > 0)),
    brain: a.groups.brain.centre,
    hands: mean(points(a, "hands").filter((p) => p[0] > 0)),
    heart: a.groups.heart.centre,
  };
};

function nerves(a: Anatomy, rnd: () => number, steps: number, eyes: V[], ears: V[]): Spec[] {
  const v = (p: V) => new THREE.Vector3(...p);
  const jit = (p: V, r: number): THREE.Vector3 => v(p).add(new THREE.Vector3((rnd() - 0.5) * r, (rnd() - 0.5) * r, 0));
  const spine = a.paths.spine;
  const root: V = [0, spine[0][1] + 0.02, spine[0][2]];
  const down = spine.filter((p) => p[1] > 0.66);
  const fibre = (pts: THREE.Vector3[]): V[] =>
    new THREE.CatmullRomCurve3(pts, false, "catmullrom", 0.3).getSpacedPoints(steps).map((p) => [p.x, p.y, p.z]);
  const hands = points(a, "hands");
  const heart = points(a, "heart");
  const pick = (list: V[]) => list[Math.floor(rnd() * list.length)];
  const out: Spec[] = [];
  for (const side of [-1, 1]) {
    const arm = a.paths[side > 0 ? "arm_left" : "arm_right"];
    for (let i = 0; i < 3; i++) {
      const eye = pick(eyes.filter((p) => p[0] * side > 0));
      out.push({
        part: PART.nerveEye,
        pts: fibre([v(eye), jit([eye[0] * 0.5, 0.835, -0.02], 0.02), jit(root, 0.02)]),
        from: [],
      });
      const ear = pick(ears.filter((p) => p[0] * side > 0));
      out.push({
        part: PART.nerveEar,
        pts: fibre([v(ear), jit([ear[0] * 0.5, 0.83, -0.05], 0.02), jit(root, 0.02)]),
        from: [],
      });
    }
    for (let i = 0; i < 4; i++) {
      const hand = pick(hands.filter((p) => p[0] * side > 0));
      const path = [jit(root, 0.015), ...down.map((p) => jit(p, 0.012)), ...arm.slice(0, 3).map((p) => jit(p, 0.03))];
      out.push({ part: PART.nerveHands, pts: fibre([...path, v(hand)]), from: [] });
      const back = arm
        .slice(0, 3)
        .reverse()
        .map((p) => jit(p, 0.03));
      out.push({ part: PART.nerveHeart, pts: fibre([...back, v(pick(heart))]), from: [] });
    }
  }
  return out;
}

/**
 * The ward's patient (`full` false: its budget times `density`) or the standing body (`full`: every point of the five
 * story organs, and the parts around them at `density`).
 */
export function buildCloud(a: Anatomy, full: boolean, density = 1, seed = 20261116): Cloud {
  const rnd = rng(seed);
  const arms: V[] = [a.paths.arm_left[2], a.paths.arm_right[2]];
  const brainBase: V = [0, a.paths.spine[0][1], a.paths.spine[0][2]];
  const heartAt = a.groups.heart.centre;
  const root = aorticRoot(heartAt);
  const arteries = points(a, "arteries");
  const { eyes, ears } = eyesAndEars(a);
  const specs: Spec[] = [
    { part: PART.skin, pts: points(a, "skin"), from: [] },
    { part: PART.bones, pts: [...points(a, "bones"), ...points(a, "urinary")], from: [] },
    {
      part: PART.muscle,
      pts: points(a, "muscles").filter((p) => Math.abs(p[0]) > 0.17 || p[1] < -0.02 || rnd() < 0.2),
      from: [],
    },
    {
      part: PART.organs,
      pts: ["lungs", "liver", "stomach", "bowel_s", "bowel_l", "gut_up"].flatMap((g) => points(a, g)),
      from: [],
    },
    // Ink enters each eye through its cornea, at the front of the eyeball.
    { part: PART.eyes, pts: eyes, from: [1, -1].map((s): V => [s * EYE[0], EYE[1], EYE[2] + EYE_RADIUS]) },
    { part: PART.ears, pts: ears, from: [mean(ears.filter((p) => p[0] > 0)), mean(ears.filter((p) => p[0] < 0))] },
    { part: PART.brain, pts: [...points(a, "brain"), ...points(a, "cord")], from: [brainBase] },
    { part: PART.hands, pts: points(a, "hands"), from: arms },
    { part: PART.heart, pts: [...points(a, "heart"), ...arteries.filter(root)], from: [heartAt] },
    { part: PART.artery, pts: arteries.filter((p) => !root(p)), from: [heartAt] },
  ];
  const sampled = specs.map((s) => ({
    ...s,
    pts: !full
      ? take(s.pts, Math.round((WARD_BUDGET[s.part] ?? 0) * density), rnd)
      : density < 1 && CONTEXT.includes(s.part)
        ? take(s.pts, Math.round(s.pts.length * density), rnd)
        : s.pts,
  }));
  const fibres = nerves(a, rnd, full ? 140 : 36, eyes, ears);
  const count = sampled.reduce((n, s) => n + s.pts.length, 0) + fibres.reduce((n, f) => n + f.pts.length, 0);
  const cloud: Cloud = {
    count,
    position: new Float32Array(count * 3),
    part: new Float32Array(count),
    u: new Float32Array(count),
    order: new Float32Array(count),
    chain: new Float32Array(count * 4),
    nrm: new Float32Array(count * 3),
    heart: heartAt,
  };
  let i = 0;
  const put = (p: V, part: Part, u: number) => {
    cloud.position.set(p, i * 3);
    cloud.nrm.set(normalOf(part, p), i * 3);
    cloud.part[i] = part;
    cloud.u[i] = u;
    cloud.order[i] = 0.72 * (1 - (p[1] + 1) / 2) + 0.28 * rnd();
    for (let r = 0; r < ROUTES.length; r++) cloud.chain[i * 4 + r] = arrival(ROUTES[r], part, u);
    i += 1;
  };
  for (const s of sampled) {
    const d = s.pts.map((p) => {
      let m = s.from.length ? Number.POSITIVE_INFINITY : 0;
      for (const f of s.from) m = Math.min(m, Math.hypot(p[0] - f[0], p[1] - f[1], p[2] - f[2]));
      return m;
    });
    let max = 1e-6;
    for (const x of d) max = Math.max(max, x);
    s.pts.forEach((p, k) => {
      put(p, s.part, d[k] / max);
    });
  }
  for (const f of fibres) {
    f.pts.forEach((p, k) => {
      put(p, f.part, k / (f.pts.length - 1));
    });
  }
  return cloud;
}

/** One geometry, drawn once per patient by instancing; per-instance attributes hold place, state and timing. */
export function cloudGeometry(cloud: Cloud, instances: number): THREE.InstancedBufferGeometry {
  const g = new THREE.InstancedBufferGeometry();
  g.setAttribute("position", new THREE.BufferAttribute(cloud.position, 3));
  g.setAttribute("part", new THREE.BufferAttribute(cloud.part, 1));
  g.setAttribute("u", new THREE.BufferAttribute(cloud.u, 1));
  g.setAttribute("order", new THREE.BufferAttribute(cloud.order, 1));
  g.setAttribute("chain", new THREE.BufferAttribute(cloud.chain, 4));
  g.setAttribute("nrm", new THREE.BufferAttribute(cloud.nrm, 3));
  const inst = (size: number) => new THREE.InstancedBufferAttribute(new Float32Array(instances * size), size);
  g.setAttribute("iOffset", inst(3));
  g.setAttribute("iScale", inst(1));
  g.setAttribute("iState", inst(4));
  // Sketch start, ink start (both long done) and ink rate: a rate of 0 would never ink.
  const time = inst(3);
  for (let k = 0; k < instances; k++) time.setXYZ(k, -100, -100, 1);
  g.setAttribute("iTime", time);
  g.instanceCount = instances;
  g.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1e9);
  return g;
}
