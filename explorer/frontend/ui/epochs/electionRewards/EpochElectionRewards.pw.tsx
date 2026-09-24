import React from 'react';

import * as celoEpochMock from 'mocks/epochs/celo';
import { test, expect } from 'playwright/lib';

import EpochElectionRewards from './EpochElectionRewards';

const number = '1234';
const hooksConfig = {
  router: {
    query: { number },
  },
};

test('base view', async({ render, mockApiResponse }) => {
  await mockApiResponse(
    'general:epoch_celo_election_rewards',
    celoEpochMock.electionRewardDetails1,
    { pathParams: { number, reward_type: 'voter' } },
  );
  const component = await render(<EpochElectionRewards data={ celoEpochMock.epoch1 }/>, { hooksConfig });
  await component.getByRole('cell', { name: 'Voting rewards' }).click();
  await expect(component).toHaveScreenshot();
});

test('base view +@mobile -@default', async({ render, mockApiResponse }) => {
  await mockApiResponse(
    'general:epoch_celo_election_rewards',
    celoEpochMock.electionRewardDetails1,
    { pathParams: { number, reward_type: 'voter' } },
  );
  const component = await render(<EpochElectionRewards data={ celoEpochMock.epoch1 }/>, { hooksConfig });
  await component.locator('div').filter({ hasText: 'Voting rewards' }).nth(3).click();
  await expect(component).toHaveScreenshot();
});
