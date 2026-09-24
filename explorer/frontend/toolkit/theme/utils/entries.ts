// https://github.com/chakra-ui/chakra-ui/blob/main/packages/react/src/utils/entries.ts#L1
export function mapEntries<A, B, K extends string | number | symbol>(
  obj: { [key in K]: A },
  f: (key: K, val: A) => [K, B],
): { [key in K]: B } {
  const result: { [key in K]: B } = {} as unknown as { [key in K]: B };
  for (const key in obj) {
    const kv = f(key, obj[key]);
    result[kv[0]] = kv[1];
  }
  return result;
}
