// MD5 of bytes, as hex: the browser has none, and the benchmark checks each picture against libvpx's `framemd5`.
// Written for speed, as a picture at 4K is 25 MB: the state in locals and no allocation in the rounds.
const S = new Int32Array([7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21]);
const K = new Int32Array(64).map((_, i) => Math.floor(Math.abs(Math.sin(i + 1)) * 2 ** 32) | 0);
const G = new Int32Array(64).map((_, i) => (i < 16 ? i : i < 32 ? (5 * i + 1) & 15 : i < 48 ? (3 * i + 5) & 15 : (7 * i) & 15));

export function md5(bytes) {
  const length = bytes.length;
  const tail = new Uint8Array(((length & 63) + 8 < 64) ? 64 : 128);
  tail.set(bytes.subarray(length & ~63));
  tail[length & 63] = 0x80;
  const tv = new DataView(tail.buffer);
  tv.setUint32(tail.length - 8, (length * 8) >>> 0, true);
  tv.setUint32(tail.length - 4, Math.floor((length * 8) / 2 ** 32), true);
  let a0 = 0x67452301, b0 = 0xefcdab89 | 0, c0 = 0x98badcfe | 0, d0 = 0x10325476;
  const m = new Int32Array(16);
  const block = (src, at) => {
    for (let i = 0; i < 16; i++) {
      const j = at + 4 * i;
      m[i] = src[j] | (src[j + 1] << 8) | (src[j + 2] << 16) | (src[j + 3] << 24);
    }
    let a = a0, b = b0, c = c0, d = d0;
    for (let i = 0; i < 64; i++) {
      const f = i < 16 ? (b & c) | (~b & d) : i < 32 ? (d & b) | (~d & c) : i < 48 ? b ^ c ^ d : c ^ (b | ~d);
      const sum = (a + f + K[i] + m[G[i]]) | 0;
      a = d; d = c; c = b;
      b = (b + ((sum << S[i]) | (sum >>> (32 - S[i])))) | 0;
    }
    a0 = (a0 + a) | 0; b0 = (b0 + b) | 0; c0 = (c0 + c) | 0; d0 = (d0 + d) | 0;
  };
  const whole = length & ~63;
  for (let at = 0; at < whole; at += 64) block(bytes, at);
  for (let at = 0; at < tail.length; at += 64) block(tail, at);
  const hex = (v) => [0, 8, 16, 24].map((s) => ((v >>> s) & 255).toString(16).padStart(2, "0")).join("");
  return hex(a0) + hex(b0) + hex(c0) + hex(d0);
}
