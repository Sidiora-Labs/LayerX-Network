export default function bytesToBase64(bytes: Uint8Array, addPrefix = true) {
  let result = '';
  for (const byte of bytes) {
    result += Number(byte).toString(16).padStart(2, '0');
  }

  return addPrefix ? `0x${ result }` : result;
}
