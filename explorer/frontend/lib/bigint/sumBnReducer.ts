import type BigNumber from 'bignumber.js';

export default function sumBnReducer(result: BigNumber, item: BigNumber) {
  return result.plus(item);
}
