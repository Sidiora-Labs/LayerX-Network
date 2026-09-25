import decodeJWT from 'lib/decodeJWT';
import * as profileMock from 'mocks/user/profile';
import { describe, expect, it, vi } from 'vitest';

import { buildRewardsApiToken, REGISTERED_ADDRESS } from './rewards';

vi.mock('configs/app', () => {
  return {
    'default': {
      app: {
        host: 'explorer.example.com',
        protocol: 'https',
      },
    },
  };
});

describe('rewards API token fixture', () => {
  it('decodes to the address the rewards context checks', () => {
    const decoded = decodeJWT(buildRewardsApiToken());

    expect(decoded?.payload.sub).toBe(REGISTERED_ADDRESS);
    expect(decoded?.header.alg).toBe('none');
  });

  it('carries the address of the mocked user profile', () => {
    expect(REGISTERED_ADDRESS).toBe(profileMock.withEmailAndWallet.address_hash);
  });

  it('decodes to the address it is built with', () => {
    const address = '0x0000000000000000000000000000000000000001';
    const decoded = decodeJWT(buildRewardsApiToken(address));

    expect(decoded?.payload.sub).toBe(address);
  });

  it('is not signed and so never looks like a credential', () => {
    // the shape tools/ci/public-repo-audit.sh rejects in the publication set
    expect(buildRewardsApiToken()).not.toMatch(/eyJ[\w-]{20,}\.[\w-]{20,}\.[\w-]{20,}/);
    expect(buildRewardsApiToken().split('.')[2]).toBe('not-a-signature');
  });
});
