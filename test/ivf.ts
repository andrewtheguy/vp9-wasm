// The frames of an IVF file: a 32-byte header, then each frame after a 12-byte
// header of its length and its time, little-endian.

export function ivfFrames(file: Uint8Array): Uint8Array[] {
  const view = new DataView(file.buffer, file.byteOffset, file.byteLength);
  const frames: Uint8Array[] = [];
  for (let at = 32; at + 12 <= file.length; ) {
    const length = view.getUint32(at, true);
    frames.push(file.subarray(at + 12, at + 12 + length));
    at += 12 + length;
  }
  return frames;
}
