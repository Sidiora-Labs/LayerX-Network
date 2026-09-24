import { useRouter } from 'next/router';

import useApiQuery from 'lib/api/useApiQuery';
import getQueryParamString from 'lib/router/getQueryParamString';
import useSocketChannel from 'lib/socket/useSocketChannel';

import useContractTabs from '../useContractTabs';

const ContractDetails = () => {
  const router = useRouter();
  const hash = getQueryParamString(router.query.hash);
  const addressQuery = useApiQuery('general:address', { pathParams: { hash } });
  const channel = useSocketChannel({
    topic: `addresses:${ hash?.toLowerCase() }`,
    isDisabled: !addressQuery.data,
  });

  const { tabs } = useContractTabs({
    addressData: addressQuery.data,
    isEnabled: true,
    channel,
  });
  const content = tabs.find(({ id }) => id === 'contract_code')?.component;
  return content ?? null;
};

export default ContractDetails;
