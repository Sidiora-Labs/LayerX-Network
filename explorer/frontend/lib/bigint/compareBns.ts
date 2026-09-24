import BigNumber from 'bignumber.js';

export default function compareBns(value1: string | number, value2: string | number) {
  const value1Bn = new BigNumber(value1);
  const value2Bn = new BigNumber(value2);
  if (value1Bn.isGreaterThan(value2Bn)) {
    return 1;
  }
  if (value1Bn.isLessThan(value2Bn)) {
    return -1;
  }
  return 0;
}
