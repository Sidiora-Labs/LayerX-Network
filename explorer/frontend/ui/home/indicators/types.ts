import type React from 'react';

import type { ChainIndicatorId } from 'types/homepage';

export interface TChainIndicator {
  id: ChainIndicatorId;
  title: string;
  titleShort?: string;
  value: string;
  valueDiff?: number;
  icon: React.ReactNode;
  hint?: string;
}
