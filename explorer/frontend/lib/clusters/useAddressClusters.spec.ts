// @vitest-environment jsdom

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook } from 'vitest/lib';

import { useAddressClusters } from './useAddressClusters';

const { mockUseApiQuery } = vi.hoisted(() => ({
  mockUseApiQuery: vi.fn(),
}));

vi.mock('lib/api/useApiQuery', () => ({
  'default': mockUseApiQuery,
}));

vi.mock('configs/app', async() => {
  return {
    'default': {
      UI: {
        colorTheme: {},
        homepage: {},
        fonts: {},
      },
      features: {
        nameServices: {
          isEnabled: true,
          ens: { isEnabled: true },
          clusters: { isEnabled: true },
        },
      },
    },
  };
});

describe('useAddressClusters', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseApiQuery.mockReturnValue({ data: null, isLoading: false });
  });

  it('should call API with correct parameters', () => {
    const addressHash = '0x1234567890123456789012345678901234567890';

    renderHook(() => useAddressClusters(addressHash));

    expect(mockUseApiQuery).toHaveBeenCalledWith('clusters:get_clusters_by_address', {
      queryParams: {
        input: JSON.stringify({
          address: addressHash,
        }),
      },
      queryOptions: {
        enabled: true,
      },
    });
  });

  it('should be disabled when addressHash is empty', () => {
    renderHook(() => useAddressClusters(''));

    expect(mockUseApiQuery).toHaveBeenCalledWith('clusters:get_clusters_by_address', {
      queryParams: {
        input: JSON.stringify({
          address: '',
        }),
      },
      queryOptions: {
        enabled: false,
      },
    });
  });

  it('should handle isEnabled parameter', () => {
    const addressHash = '0x1234567890123456789012345678901234567890';

    renderHook(() => useAddressClusters(addressHash, false));

    expect(mockUseApiQuery).toHaveBeenCalledWith('clusters:get_clusters_by_address', {
      queryParams: {
        input: JSON.stringify({
          address: addressHash,
        }),
      },
      queryOptions: {
        enabled: false,
      },
    });
  });
});
