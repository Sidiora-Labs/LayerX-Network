// hex can be with prefix  - `0x{string}` - or without it - `{string}`
export default function hexToBytes(hex: string) {
  const bytes = [];
  const startIndex = hex.startsWith('0x') ? 2 : 0;
  for (let c = startIndex; c < hex.length; c += 2) {
    bytes.push(parseInt(hex.substring(c, c + 2), 16));
  }
  return new Uint8Array(bytes);
}
