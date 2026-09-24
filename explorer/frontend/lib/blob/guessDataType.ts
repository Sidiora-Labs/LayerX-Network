import filetype from 'magic-bytes.js';

import hexToBytes from 'lib/hexToBytes';

import removeNonSignificantZeroBytes from './removeNonSignificantZeroBytes';

export default function guessDataType(data: string) {
  const bytes = hexToBytes(data);
  const filteredBytes = removeNonSignificantZeroBytes(bytes);

  return filetype(filteredBytes)[0];
}
