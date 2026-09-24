import { bech32 } from '@scure/base';

import config from 'configs/app';
import bytesToHex from 'lib/bytesToHex';
import hexToBytes from 'lib/hexToBytes';

export const DATA_PART_REGEXP = /^[\da-z]{38}$/;
export const BECH_32_SEPARATOR = '1'; // https://github.com/bitcoin/bips/blob/master/bip-0173.mediawiki#bech32

export function toBech32Address(hash: string) {
  if (config.UI.views.address.hashFormat.bech32Prefix) {
    try {
      const words = bech32.toWords(hexToBytes(hash));
      return bech32.encode(config.UI.views.address.hashFormat.bech32Prefix, words);
    } catch (error) {}
  }

  return hash;
}

export function isBech32Address(hash: string) {
  if (!config.UI.views.address.hashFormat.bech32Prefix) {
    return false;
  }

  if (!hash.startsWith(`${ config.UI.views.address.hashFormat.bech32Prefix }${ BECH_32_SEPARATOR }`)) {
    return false;
  }

  const strippedHash = hash.replace(`${ config.UI.views.address.hashFormat.bech32Prefix }${ BECH_32_SEPARATOR }`, '');
  return DATA_PART_REGEXP.test(strippedHash);
}

export function fromBech32Address(hash: string) {
  if (config.UI.views.address.hashFormat.bech32Prefix) {
    try {
      const { words, prefix } = bech32.decode(hash as `${ string }${ typeof BECH_32_SEPARATOR }${ string }`);

      if (prefix !== config.UI.views.address.hashFormat.bech32Prefix) {
        return hash;
      }

      const bytes = bech32.fromWords(words);
      return bytesToHex(bytes);
    } catch (error) {}
  }

  return hash;
}
