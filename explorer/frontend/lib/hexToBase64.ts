import bytesToBase64 from './bytesToBase64';
import hexToBytes from './hexToBytes';

export default function hexToBase64(hex: string) {
  const bytes = hexToBytes(hex);

  return bytesToBase64(bytes);
}
