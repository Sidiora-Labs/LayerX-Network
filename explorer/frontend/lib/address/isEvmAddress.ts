import { ADDRESS_REGEXP } from 'toolkit/utils/regexp';

export function isEvmAddress(address: string): boolean {
  if (!address) return false;
  return ADDRESS_REGEXP.test(address.trim());
}
