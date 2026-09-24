export function getHexadecimalChainId(chainId: number) {
  return '0x' + Number(chainId).toString(16);
}
