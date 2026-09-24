export default function hexToAddress(hex: string) {
  const shortenHex = hex.slice(0, 66);
  return shortenHex.slice(0, 2) + shortenHex.slice(26);
}
