import { it, expect } from 'vitest';

import { extractVariables, getStringChunks, checkSummary } from './utils';

const template = '{action_type} {source_amount} {native} into {destination_amount} {destination_token}';

it('extracts variables names', () => {
  const result = extractVariables(template);
  expect(result).toEqual([ 'action_type', 'source_amount', 'native', 'destination_amount', 'destination_token' ]);
});

it('split string without capturing variables', () => {
  const result = getStringChunks(template);
  expect(result).toEqual([ '', ' ', ' ', ' into ', ' ', '' ]);
});

it('checks that summary is valid', () => {
  const result = checkSummary('{foo} {native} {bar} {wei}', { foo: { type: 'string', value: 'foo' }, bar: { type: 'string', value: 'bar' } });
  expect(result).toBe(true);
});

it('checks that summary is invalid', () => {

  // @ts-ignore:
  const result = checkSummary('{foo} {native} {bar}', { foo: { type: 'string', value: null }, bar: { type: 'string', value: 'bar' } });
  expect(result).toBe(false);
});
