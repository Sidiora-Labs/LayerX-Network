import type { ZetaChainCCTXFilterParams } from 'types/client/zetaChain';

import ZetaChainAddressFilter from './ZetaChainAddressFilter';

const FILTER_PARAM_SENDER = 'sender_address';
const FILTER_PARAM_SENDER_CHAIN = 'source_chain_id';

type Props = {
  value?: Array<string>;
  chainValue?: Array<string>;
  handleFilterChange: (field: keyof ZetaChainCCTXFilterParams, value?: Array<string> | Array<string>) => void;
  columnName: string;
  isLoading?: boolean;
  onClose?: () => void;
};

const ZetaChainSenderFilter = (props: Props) => {
  return (
    <ZetaChainAddressFilter
      { ...props }
      filterParam={ FILTER_PARAM_SENDER }
      chainFilterParam={ FILTER_PARAM_SENDER_CHAIN }
      title="Sender"
      placeholder="Sender address"
    />
  );
};

export default ZetaChainSenderFilter;
