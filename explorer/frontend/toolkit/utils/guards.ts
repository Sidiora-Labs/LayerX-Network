export function castToString(payload: unknown) {
  return typeof payload === 'string' ? payload : undefined;
}

export function castToNumber(payload: unknown) {
  return typeof payload === 'number' ? payload : undefined;
}
