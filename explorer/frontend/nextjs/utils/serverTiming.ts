import type { ServerResponse } from 'http';

export function appendValue(res: ServerResponse | undefined, name: string, value: number) {
  const currentValue = res?.getHeader('Server-Timing') || '';
  const nextValue = [
    currentValue,
    `${ name };dur=${ value }`,
  ].filter(Boolean).join(',');
  res?.setHeader('Server-Timing', nextValue);
}
