import { ADDRESS_REGEXP } from 'toolkit/utils/regexp';

export const ADDRESS_LENGTH = 42;

export function addressValidator(value: string | undefined) {
  if (!value) {
    return true;
  }

  return ADDRESS_REGEXP.test(value) ? true : 'Incorrect address format';
}
