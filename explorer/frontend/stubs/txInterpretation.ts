import type { TxInterpretationResponse } from 'types/api/txInterpretation';

import { TOKEN_INFO_ERC_20 } from './token';

export const TX_INTERPRETATION: TxInterpretationResponse = {
  data: {
    summaries: [
      {
        summary_template: '{action_type} {source_amount} Ether into {destination_amount} {destination_token}',
        summary_template_variables: {
          action_type: { type: 'string', value: 'Wrap' },
          source_amount: { type: 'currency', value: '0.7' },
          destination_amount: { type: 'currency', value: '0.7' },
          destination_token: {
            type: 'token',
            value: TOKEN_INFO_ERC_20,
          },
        },
      },
      {
        summary_template: '{action_type} {source_amount} Ether into {destination_amount} {destination_token}',
        summary_template_variables: {
          action_type: { type: 'string', value: 'Wrap' },
          source_amount: { type: 'currency', value: '0.7' },
          destination_amount: { type: 'currency', value: '0.7' },
          destination_token: {
            type: 'token',
            value: TOKEN_INFO_ERC_20,
          },
        },
      },
    ],
  },
};
