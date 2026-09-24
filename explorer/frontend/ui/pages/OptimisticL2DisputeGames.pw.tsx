import React from 'react';

import { data as disputeGamesData } from 'mocks/optimism/disputeGames';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import OptimisticL2DisputeGames from './OptimisticL2DisputeGames';

test('base view +@mobile', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  test.slow();
  await mockEnvs(ENVS_MAP.optimisticRollup);
  await mockTextAd();
  await mockApiResponse('general:optimistic_l2_dispute_games', disputeGamesData);
  await mockApiResponse('general:optimistic_l2_dispute_games_count', 3971111);

  const component = await render(<OptimisticL2DisputeGames/>);

  await expect(component).toHaveScreenshot();
});
