import { isEvmAddress } from 'lib/address/isEvmAddress';
import { describe, it, expect } from 'vitest';

import { detectInputType } from './detectInputType';

describe('detectInputType', () => {
  it('should detect EVM address format', () => {
    expect(detectInputType('0x1234567890123456789012345678901234567890')).toBe('address');
  });

  it('should detect cluster name format', () => {
    expect(detectInputType('test-cluster')).toBe('cluster_name');
  });
});

describe('isEvmAddress', () => {
  it('should return true for valid EVM address', () => {
    expect(isEvmAddress('0x1234567890123456789012345678901234567890')).toBe(true);
    expect(isEvmAddress('0xabcdef1234567890123456789012345678901234')).toBe(true);
    expect(isEvmAddress('0xABCDEF1234567890123456789012345678901234')).toBe(true);
  });

  it('should return false for invalid EVM address', () => {
    expect(isEvmAddress('0x123')).toBe(false);
    expect(isEvmAddress('123456789012345678901234567890123456789')).toBe(false);
    expect(isEvmAddress('0xGGGGGG1234567890123456789012345678901234')).toBe(false);
    expect(isEvmAddress('0x12345678901234567890123456789012345678901')).toBe(false);
  });

  it('should return false for empty or null input', () => {
    expect(isEvmAddress('')).toBe(false);
    expect(isEvmAddress(null as unknown as string)).toBe(false);
    expect(isEvmAddress(undefined as unknown as string)).toBe(false);
  });

  it('should handle addresses with extra whitespace', () => {
    expect(isEvmAddress('  0x1234567890123456789012345678901234567890  ')).toBe(true);
    expect(isEvmAddress(' 0x123 ')).toBe(false);
  });
});
