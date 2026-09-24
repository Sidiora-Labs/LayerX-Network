import type { InterchainTransfer } from '@blockscout/interchain-indexer-types';

export const getItemKey = (data: InterchainTransfer, index?: number) => {
  return [
    data.message_id,
    data.sender?.hash,
    data.source_token?.address_hash,
    data.source_amount,
    data.source_chain?.id,
    index,
  ]
    .filter((item) => item !== undefined)
    .join('-');
};
