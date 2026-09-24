import type { ApiResource } from '../types';
import type * as visualizer from '@blockscout/visualizer-types';

export const VISUALIZE_API_RESOURCES = {
  solidity_contract: {
    path: '/api/v1/solidity\\:visualize-contracts',
  },
} satisfies Record<string, ApiResource>;

export type VisualizeApiResourceName = `visualize:${ keyof typeof VISUALIZE_API_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type VisualizeApiResourcePayload<R extends VisualizeApiResourceName> =
R extends 'visualize:solidity_contract' ? visualizer.VisualizeResponse :
never;
/* eslint-enable @stylistic/indent */
