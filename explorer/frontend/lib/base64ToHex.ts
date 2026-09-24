import bytesToHex from './bytesToHex';

export default function base64ToHex(base64: string): string {
  const bytes = Uint8Array.from(atob(base64), c => c.charCodeAt(0));
  return bytesToHex(bytes, false);
}
