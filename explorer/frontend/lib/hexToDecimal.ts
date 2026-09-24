export default function hetToDecimal(hex: string) {
  const strippedHex = hex.startsWith('0x') ? hex.slice(2) : hex;
  return parseInt(strippedHex, 16);
}
