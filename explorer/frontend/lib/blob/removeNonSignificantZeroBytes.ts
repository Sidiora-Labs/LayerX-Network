export default function removeNonSignificantZeroBytes(bytes: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
  return shouldRemoveBytes(bytes) ? bytes.filter((item, index) => index % 32) : bytes;
}

// check if every 0, 32, 64, etc byte is 0 in the provided array
function shouldRemoveBytes(bytes: Uint8Array) {
  let result = true;

  for (let index = 0; index < bytes.length; index += 32) {
    const element = bytes[index];
    if (element === 0) {
      continue;
    } else {
      result = false;
      break;
    }
  }

  return result;
}
