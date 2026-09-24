import config from 'configs/app';
import { INTERCHAIN_TRANSFER } from 'stubs/interchainIndexer';
import { generateListStub } from 'stubs/utils';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

interface Props {
  hash: string;
  enabled?: boolean;
}

export default function useTxCrossChainTransfersQuery({ hash, enabled = true }: Props) {
  return useQueryWithPages({
    resourceName: 'interchainIndexer:tx_transfers',
    pathParams: { hash },
    options: {
      placeholderData: generateListStub<'interchainIndexer:tx_transfers'>(INTERCHAIN_TRANSFER, 5, { next_page_params: undefined }),
      enabled: enabled && config.features.crossChainTxs.isEnabled,
      staleTime: Infinity,
    },
  });
}
