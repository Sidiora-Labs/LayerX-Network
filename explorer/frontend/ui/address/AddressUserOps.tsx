import { useRouter } from 'next/router';
import React from 'react';

import useIsMounted from 'lib/hooks/useIsMounted';
import getQueryParamString from 'lib/router/getQueryParamString';
import { USER_OPS_ITEM } from 'stubs/userOps';
import { generateListStub } from 'stubs/utils';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import UserOpsContent from 'ui/userOps/UserOpsContent';

type Props = {
  scrollRef?: React.RefObject<HTMLDivElement>;
  shouldRender?: boolean;
  isQueryEnabled?: boolean;
};

const AddressUserOps = ({ scrollRef, shouldRender = true, isQueryEnabled = true }: Props) => {
  const router = useRouter();
  const isMounted = useIsMounted();

  const hash = getQueryParamString(router.query.hash);

  const userOpsQuery = useQueryWithPages({
    resourceName: 'general:user_ops',
    scrollRef,
    options: {
      enabled: isQueryEnabled && Boolean(hash),
      placeholderData: generateListStub<'general:user_ops'>(USER_OPS_ITEM, 50, { next_page_params: {
        page_token: '10355938,0x5956a847d8089e254e02e5111cad6992b99ceb9e5c2dc4343fd53002834c4dc6',
        page_size: 50,
      } }),
    },
    filters: { sender: hash },
  });

  if (!isMounted || !shouldRender) {
    return null;
  }

  return <UserOpsContent query={ userOpsQuery } showSender={ false }/>;
};

export default AddressUserOps;
