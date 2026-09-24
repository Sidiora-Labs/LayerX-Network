import { ADDRESS_REGEXP } from 'toolkit/utils/regexp';

export type InputType = 'address' | 'cluster_name';

export function detectInputType(input: string): InputType {
  if (!input || input.trim().length === 0) {
    return 'cluster_name';
  }

  const trimmedInput = input.trim();

  if (ADDRESS_REGEXP.test(trimmedInput)) {
    return 'address';
  }

  return 'cluster_name';
}
