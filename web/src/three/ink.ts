import * as THREE from "three";
import type { Route } from "../record/types";

/** What a point of the body is. Pencil parts never take ink; the rest sit on an infection's chain. */
export const PART = {
  skin: 0,
  bones: 1,
  muscle: 2,
  organs: 3,
  eyes: 4,
  ears: 5,
  brain: 6,
  hands: 7,
  heart: 8,
  artery: 9,
  nerveEye: 10,
  nerveEar: 11,
  nerveHands: 12,
  nerveHeart: 13,
} as const;
export type Part = (typeof PART)[keyof typeof PART];

export const ROUTES: Route[] = ["EYE", "EAR", "MEM", "SKL"];

const TAIL: [Part, number, number][] = [
  [PART.nerveHeart, 5, 6],
  [PART.heart, 6, 6.6],
  [PART.artery, 6.6, 7],
];

/**
 * The chain an infection inks, per entry route: entry organ, nerve to the brain, brain, nerve to the hands, hands,
 * nerve to the heart, heart and its arteries. A span runs from `from` at u = 0 to `to` at u = 1.
 */
const CHAINS: Record<Route, [Part, number, number][]> = {
  EYE: [
    [PART.eyes, 0, 1],
    [PART.nerveEye, 1, 2],
    [PART.brain, 2, 3],
    [PART.nerveHands, 3, 4],
    [PART.hands, 4, 5],
    ...TAIL,
  ],
  EAR: [
    [PART.ears, 0, 1],
    [PART.nerveEar, 1, 2],
    [PART.brain, 2, 3],
    [PART.nerveHands, 3, 4],
    [PART.hands, 4, 5],
    ...TAIL,
  ],
  MEM: [[PART.brain, 0, 3], [PART.nerveHands, 3, 4], [PART.hands, 4, 5], ...TAIL],
  SKL: [[PART.hands, 0, 1], [PART.nerveHands, 2, 1], [PART.brain, 2, 3], ...TAIL],
};

/** Where a point of `part` at `u` sits on the chain of `route`, or -1 when that chain never reaches it. */
export function chainAt(route: Route, part: Part, u: number): number {
  const span = CHAINS[route].find(([p]) => p === part);
  return span ? span[1] + u * (span[2] - span[1]) : -1;
}

/** The front's timetable at rate 1 (the patient page): story seconds at chain positions 0 to 7, and at 6.6. */
export const KNOTS: readonly (readonly [number, number])[] = [
  [0, 0],
  [1, 0.4],
  [2, 0.75],
  [3, 1.3],
  [4, 1.85],
  [5, 2.25],
  [6, 2.7],
  [6.6, 3.1],
  [7, 3.6],
];

/** Story seconds at chain position `c`, straight between knots. */
export function knotTime(c: number): number {
  for (let i = 1; i < KNOTS.length; i++) {
    const [c0, t0] = KNOTS[i - 1];
    const [c1, t1] = KNOTS[i];
    if (c <= c1) return t0 + ((c - c0) / (c1 - c0)) * (t1 - t0);
  }
  return KNOTS[KNOTS.length - 1][1];
}

const NERVES: number[] = [PART.nerveEye, PART.nerveEar, PART.nerveHands, PART.nerveHeart];

/**
 * Story seconds after the ink starts when the front reaches a point of `part` at `u`, or -1 off the chain. Nerves
 * conduct evenly; an organ fills like ink in paper, fast at the way in and slower toward its far edge (u squared).
 */
export function arrival(route: Route, part: Part, u: number): number {
  const span = CHAINS[route].find(([p]) => p === part);
  if (!span) return -1;
  const f = NERVES.includes(part) ? u : u * u;
  return knotTime(span[1] + f * (span[2] - span[1]));
}

/** When the ink of a body that reached `reach` (record/stats reachOf: 1, 3, 5, 7) stops; -1 for one it never reached. */
export const reachTime = (reach: number) => (reach > 0 ? knotTime(reach) : -1);

export const COLOURS = {
  paper: "#ECEBE8",
  graphite: "#1E2026",
  eosin: "#E5306F",
  violet: "#4B3A9E",
  steel: "#6F767F",
  umber: "#7A4E2C",
};

/** Rest opacity per part (PART order), for the 48k-point brain and the 3k-point ears in public/anatomy. */
export const PENCIL = [0.3, 0.46, 0.14, 0.16, 0.75, 0.45, 0.12, 0.42, 0.26, 0.12, 0.07, 0.07, 0.07, 0.07];
/** The ward's rest opacity: a grey grid whose heads and chests read darker, so pink is the only colour at rest. */
const WARD_PENCIL = [0.3, 0.5, 0.14, 0.2, 0.6, 0.5, 0.42, 0.36, 0.5, 0.12, 0.07, 0.07, 0.07, 0.07];

/** Parts that only give the body its shape: the ones a phone draws at lower density. */
export const CONTEXT: Part[] = [PART.skin, PART.bones, PART.muscle, PART.organs, PART.artery];

const vertex = /* glsl */ `
uniform float uTime;
uniform float uSize;
uniform float uZoom;
uniform float uLens;
uniform float uMotion;
uniform float uRoll;
uniform float uYaw;
uniform vec2 uLook;
uniform float uRollPhase;
uniform float uBeat;
uniform float uShimmer;
uniform vec3 uHeart;
uniform float uFocusPart;
uniform float uFocus;
uniform vec2 uOrgan;
uniform vec2 uRim;
uniform vec2 uDepth;
uniform float uDrain;
uniform float uSolo;
uniform float uSoloFade;
uniform vec3 uPose;
uniform mat3 uOrient;
uniform float uPencil[14];
uniform vec3 uGraphite;
uniform vec3 uEosin;
uniform vec3 uViolet;
uniform vec3 uSteel;
uniform vec3 uUmber;
uniform float uTint;
uniform float uTintWho;
attribute float part;
attribute float u;
attribute float order;
attribute vec4 chain;
attribute vec3 nrm;
attribute vec3 iOffset;
attribute float iScale;
attribute vec4 iState;
attribute vec3 iTime;
varying vec4 vColour;

float easeInOut(float t) {
  return t < 0.5 ? 4.0 * t * t * t : 1.0 - pow(-2.0 * t + 2.0, 3.0) / 2.0;
}

void main() {
  float reachT = iState.x;
  int marks = int(iState.y + 0.5);
  float isVoid = iState.z;
  int route = int(iState.w + 0.5);
  float arrive = route == 0 ? chain.x : route == 1 ? chain.y : route == 2 ? chain.z : chain.w;
  int kind = int(part + 0.5);
  bool organ = kind >= ${PART.eyes} && kind <= ${PART.heart};
  float me = float(gl_InstanceID);

  // a resting heartbeat, lub-dub at 60 bpm, about the heart's centre
  vec3 p = position;
  float beat = 0.0;
  if (kind == ${PART.heart}) {
    float ph = fract(uTime);
    beat = uMotion * (exp(-pow((ph - 0.08) / 0.045, 2.0)) + 0.55 * exp(-pow((ph - 0.3) / 0.055, 2.0)));
    p = uHeart + (p - uHeart) * (1.0 + uBeat * beat);
  }

  // the idle roll about the long axis, plus a close-up's fixed turn and the ward's lean toward the pointer (uLook.y);
  // a patient lying down blends from its old roll
  float roll = uYaw + uLook.y + uRoll * sin(uTime * uMotion * 0.35 + uRollPhase + iOffset.x * 0.013 + iOffset.y * 0.021);
  float lie = 1.0;
  if (me == uPose.x) {
    lie = easeInOut(clamp((uTime - uPose.y) / 0.6, 0.0, 1.0));
    roll = mix(uPose.z, roll, lie);
  }
  float cr = cos(roll);
  float sr = sin(roll);
  p = vec3(p.x * cr - p.z * sr, p.y, p.x * sr + p.z * cr);
  float facing = nrm.x * sr + nrm.z * cr;
  float rim = dot(nrm, nrm) > 0.5 ? pow(1.0 - abs(facing), 1.5) : 0.5;
  float depth = smoothstep(-0.12, 0.1, p.z);
  // the ward's lean about the short axis, toward a pointer to the side (zero on the body page)
  p = vec3(p.x, p.y * cos(uLook.x) - p.z * sin(uLook.x), p.y * sin(uLook.x) + p.z * cos(uLook.x));
  if (lie < 1.0) {
    float a = -1.5708 * (1.0 - lie);
    p = vec3(p.x * cos(a) - p.y * sin(a), p.x * sin(a) + p.y * cos(a), p.z);
  }
  vec3 world = uOrient * p * iScale + iOffset;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(world, 1.0);

  // the pencil sketches head to feet; the organs warm to umber a beat after the pencil passes them
  float since = (uTime - iTime.x) * 0.75 - order;
  float drawn = smoothstep(0.0, 0.08, since);
  float develop = smoothstep(0.2, 0.45, since);

  // the front: story seconds since the ink started, at this patient's rate. A run that never reached the brain
  // (mark 2) stains it no further than the way in: the venom runs past along the nerves and the brain stays umber.
  float elapsed = (uTime - iTime.y) * iTime.z;
  float reachHere = kind == ${PART.brain} && marks >= 2 ? min(reachT, ${reachTime(1).toFixed(3)}) : reachT;
  bool onChain = arrive >= 0.0 && arrive <= reachHere + 0.0001 && isVoid < 0.5;
  float age = elapsed - arrive;
  float inked = onChain && age >= 0.0 ? 1.0 - uDrain : 0.0;
  float wet = inked * (1.0 - smoothstep(0.0, 0.32, age));
  float lead = onChain && age < 0.0 ? smoothstep(-0.14, 0.0, age) * (1.0 - uDrain) : 0.0;

  // at rest: graphite pencil; organs warm to umber (uTint) with a contour and a lighter back. The body page warms
  // every point; the ward only the patient flying to or from its page (uTintWho), so the grid stays grey.
  float tint = uTint * (uTintWho < -0.5 || abs(me - uTintWho) < 0.5 ? 1.0 : 0.0);
  vec3 rest = organ ? mix(uGraphite, uUmber, develop * tint) : uGraphite;
  float a = uPencil[kind] * mix(organ ? uDepth.y : uDepth.x, 1.0, depth);
  if (organ) a *= uOrgan.x * clamp(pow(uLens, 0.6), 1.0, 2.6) * mix(uRim.x, uRim.y, rim);
  a *= 1.0 + 0.12 * beat;
  if (kind == ${PART.brain}) a *= 1.0 + uShimmer * uMotion * sin(6.2832 * uTime / 4.0 - position.y * 22.0 - position.z * 26.0);
  bool other = (organ || part > 9.5) && abs(part - uFocusPart) >= 0.5;
  a *= mix(1.0, organ ? (other ? 0.55 : 1.3) : 0.6, uFocus);
  rest = mix(rest, uSteel, isVoid);
  a *= mix(1.0, 0.75, isVoid);

  // ink: eosin where the attack reached; hands a gene held (mark 1) scar violet once the front stops
  float scar = (marks == 1 || marks == 3) && kind == ${PART.hands} ? smoothstep(reachT + 0.15, reachT + 0.75, elapsed) : 0.0;
  vec3 ink = mix(uEosin, uViolet, scar);
  float inkA = kind == ${PART.artery} ? 0.5 : organ ? min(1.0, 0.85 * mix(uDepth.y, 1.0, depth) * mix(uRim.x, uRim.y, rim)) : 0.92;
  vec3 colour = mix(mix(rest, uEosin, 0.5 * lead), ink, inked);
  float alpha = mix(mix(a, 0.7, lead), inkA, inked);
  // a close-up keeps its organ the strongest ink in frame: inked neighbours and nerves recede with the rest
  alpha *= mix(1.0, other ? 0.45 : 1.0, uFocus * inked);
  alpha *= me == uSolo ? 1.0 : 1.0 - uSoloFade;
  float swell = inked * (kind == ${PART.eyes} || kind == ${PART.ears} || kind == ${PART.artery} ? 0.1
    : kind == ${PART.brain} || kind == ${PART.heart} ? 0.2 : 0.45);
  vColour = vec4(colour, min(alpha, 1.0) * drawn);
  gl_PointSize = uSize * pow(uZoom, 0.55) * (1.0 + swell + 0.9 * wet + 0.35 * lead) * (part > 9.5 ? 1.15 : organ ? uOrgan.y : 1.0);
}
`;

const fragment = /* glsl */ `
varying vec4 vColour;
void main() {
  vec2 d = gl_PointCoord - 0.5;
  float r = dot(d, d);
  if (r > 0.25 || vColour.a < 0.004) discard;
  gl_FragColor = vec4(vColour.rgb, vColour.a * smoothstep(0.25, 0.14, r));
  #include <colorspace_fragment>
}
`;

/**
 * Per view: the ward draws small graphite bodies (no contour or depth, no beat; one patient's organs warm to umber
 * only on its way to or from its page), the body page a large one with umber organs.
 */
const VIEW = {
  ward: { pencil: WARD_PENCIL, organ: [1, 1], rim: [1, 1], depth: [1, 1], beat: 0, shimmer: 0, tint: 0 },
  body: { pencil: PENCIL, organ: [1, 1], rim: [0.5, 1.6], depth: [0.8, 0.5], beat: 0.025, shimmer: 0.1, tint: 1 },
} as const;

/** Idle roll about the long axis, in radians: the ward's beds and the standing body. */
export const ROLL = 0.28;

/**
 * The point material for one view; `heart` is the heartbeat's pivot (Cloud.heart). A cloud drawn at `density` below
 * 1 darkens its context parts to match, so a phone's thinner body keeps the pencil's tone.
 */
export function inkMaterial(
  size: number,
  view: keyof typeof VIEW,
  heart: readonly number[],
  density = 1,
): THREE.ShaderMaterial {
  const c = (hex: string) => new THREE.Color(hex);
  const v = VIEW[view];
  const pencil = v.pencil.map((a, k) => (CONTEXT.includes(k as Part) ? Math.min(1, a / density) : a));
  return new THREE.ShaderMaterial({
    vertexShader: vertex,
    fragmentShader: fragment,
    transparent: true,
    depthWrite: false,
    uniforms: {
      uTime: { value: 0 },
      uSize: { value: size },
      uZoom: { value: 1 },
      uLens: { value: 1 },
      uMotion: { value: 1 },
      uRoll: { value: ROLL },
      uYaw: { value: 0 },
      uLook: { value: new THREE.Vector2() },
      uRollPhase: { value: 0 },
      uBeat: { value: v.beat },
      uShimmer: { value: v.shimmer },
      uHeart: { value: new THREE.Vector3(heart[0], heart[1], heart[2]) },
      uFocusPart: { value: -1 },
      uFocus: { value: 0 },
      uOrgan: { value: new THREE.Vector2(...v.organ) },
      uRim: { value: new THREE.Vector2(...v.rim) },
      uDepth: { value: new THREE.Vector2(...v.depth) },
      uDrain: { value: 0 },
      uSolo: { value: -1 },
      uSoloFade: { value: 0 },
      uPose: { value: new THREE.Vector3(-1, -100, 0) },
      uOrient: { value: new THREE.Matrix3() },
      uPencil: { value: pencil },
      uGraphite: { value: c(COLOURS.graphite) },
      uEosin: { value: c(COLOURS.eosin) },
      uViolet: { value: c(COLOURS.violet) },
      uSteel: { value: c(COLOURS.steel) },
      uUmber: { value: c(COLOURS.umber) },
      uTint: { value: v.tint },
      uTintWho: { value: -1 },
    },
  });
}
