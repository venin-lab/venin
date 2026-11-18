// The core's seeds and random stream (docs/RECORD.md, Randomness), synchronous so a pick needs no await.

const K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
  0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
  0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
  0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
  0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
  0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
  0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
  0xc67178f2,
]);

export function sha256(data: Uint8Array): Uint8Array {
  const bits = data.length * 8;
  const padded = new Uint8Array(Math.ceil((data.length + 9) / 64) * 64);
  padded.set(data);
  padded[data.length] = 0x80;
  const view = new DataView(padded.buffer);
  view.setUint32(padded.length - 8, Math.floor(bits / 2 ** 32));
  view.setUint32(padded.length - 4, bits >>> 0);
  const h = new Uint32Array([
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ]);
  const w = new Uint32Array(64);
  const rotr = (x: number, n: number) => (x >>> n) | (x << (32 - n));
  for (let off = 0; off < padded.length; off += 64) {
    for (let i = 0; i < 16; i++) w[i] = view.getUint32(off + i * 4);
    for (let i = 16; i < 64; i++) {
      const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) | 0;
    }
    const v = Array.from(h);
    for (let i = 0; i < 64; i++) {
      const [a, b, c, , e, f, g, hh] = v;
      const t1 = (hh + (rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)) + ((e & f) ^ (~e & g)) + K[i] + w[i]) | 0;
      const t2 = ((rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)) + ((a & b) ^ (a & c) ^ (b & c))) | 0;
      v.pop();
      v.unshift((t1 + t2) | 0);
      v[4] = (v[4] + t1) | 0;
    }
    for (let i = 0; i < 8; i++) h[i] = (h[i] + v[i]) | 0;
  }
  const out = new Uint8Array(32);
  const outView = new DataView(out.buffer);
  for (let i = 0; i < 8; i++) outView.setUint32(i * 4, h[i]);
  return out;
}

function concat(...parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

function hmac(key: Uint8Array, message: Uint8Array): Uint8Array {
  const block = new Uint8Array(64);
  block.set(key.length > 64 ? sha256(key) : key);
  const inner = block.map((b) => b ^ 0x36);
  const outer = block.map((b) => b ^ 0x5c);
  return sha256(concat(outer, sha256(concat(inner, message))));
}

const ascii = (s: string) => Uint8Array.from(s, (c) => c.charCodeAt(0));

export const hex = (bytes: Uint8Array) => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");

export function fromHex(text: string): Uint8Array {
  const clean = text.startsWith("0x") ? text.slice(2) : text;
  return Uint8Array.from({ length: clean.length / 2 }, (_, i) => Number.parseInt(clean.slice(i * 2, i * 2 + 2), 16));
}

/** HKDF-SHA256 with salt `venin-v1`, 32 bytes out. */
export function derive(ikm: Uint8Array, info: Uint8Array): Uint8Array {
  const prk = hmac(ascii("venin-v1"), ikm);
  return hmac(prk, concat(info, Uint8Array.of(1)));
}

function u64be(n: number): Uint8Array {
  const out = new Uint8Array(8);
  new DataView(out.buffer).setBigUint64(0, BigInt(n));
  return out;
}

export function seed(drawHash: Uint8Array, n: number, label: string): Uint8Array {
  return derive(drawHash, concat(u64be(n), ascii(label)));
}

export class Stream {
  private counter = 0;
  private block: Uint8Array = new Uint8Array(32);
  private used = 32;

  constructor(private readonly seedBytes: Uint8Array) {}

  byte(): number {
    if (this.used === 32) {
      this.block = sha256(concat(this.seedBytes, u64be(this.counter)));
      this.counter += 1;
      this.used = 0;
    }
    this.used += 1;
    return this.block[this.used - 1];
  }

  nextU64(): bigint {
    let x = 0n;
    for (let i = 0; i < 8; i++) x = (x << 8n) | BigInt(this.byte());
    return x;
  }

  below(k: number): number {
    const kk = BigInt(k);
    const floor = ((1n << 64n) - kk) % kk;
    for (;;) {
      const x = this.nextU64();
      if (x >= floor) return Number(x % kk);
    }
  }

  pick<T>(items: readonly T[]): T | undefined {
    return items.length === 0 ? undefined : items[this.below(items.length)];
  }
}
