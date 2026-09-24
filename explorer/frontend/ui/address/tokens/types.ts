import type { AddressTokenBalance } from 'types/api/address';

export type AddressTokensErc20Item = Pick<AddressTokenBalance, 'token' | 'value'> & {
  chain_values?: Record<string, string>;
};
