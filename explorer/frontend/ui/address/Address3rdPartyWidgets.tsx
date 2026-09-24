import { Grid, Flex } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import { useMemo } from 'react';

import type { Address3rdPartyWidget } from 'types/views/address';

import { route } from 'nextjs-routes';

import useIsMounted from 'lib/hooks/useIsMounted';
import getQueryParamString from 'lib/router/getQueryParamString';
import { Link } from 'toolkit/chakra/link';

import Address3rdPartyWidgetCard from './address3rdPartyWidgets/Address3rdPartyWidgetCard';
import useAddress3rdPartyWidgets from './address3rdPartyWidgets/useAddress3rdPartyWidgets';

type Props = {
  shouldRender?: boolean;
  isQueryEnabled?: boolean;
  addressType: Address3rdPartyWidget['pages'][number];
  showAll?: boolean;
  isLoading?: boolean;
};

const NUMBER_OF_WIDGETS_TO_DISPLAY = 8;

const Address3rdPartyWidgets = ({ shouldRender = true, isQueryEnabled = true, addressType, isLoading = false, showAll }: Props) => {
  const router = useRouter();
  const isMounted = useIsMounted();

  const addressHash = getQueryParamString(router.query.hash);

  const { items: widgets, configQuery } = useAddress3rdPartyWidgets(addressType, isLoading, isQueryEnabled);

  const displayedWidgets = useMemo(() => {
    return showAll ? widgets : widgets.slice(0, NUMBER_OF_WIDGETS_TO_DISPLAY);
  }, [ widgets, showAll ]);

  const shouldShowViewAllLink = !showAll && !isLoading && !configQuery.isPlaceholderData && widgets.length > displayedWidgets.length;

  if (!isMounted || !shouldRender) {
    return null;
  }

  return (
    <Flex w="full" direction="column" alignItems="flex-start" gap={ 3 }>
      <Grid
        gap={ 3 }
        templateColumns={{
          base: 'repeat(auto-fit, minmax(238px, 1fr))',
          xl: 'repeat(auto-fit, minmax(248px, 1fr))',
        }}
        w="full"
        maxW={
          widgets.length < 5 ?
            `${ (widgets.length * (360 + 12)) - 12 }px` : // 360px - max widget width, 12px - gap
            undefined
        }
      >
        { displayedWidgets.map((name) => (
          <Address3rdPartyWidgetCard
            key={ name }
            name={ name }
            config={ configQuery.data?.[name] }
            address={ addressHash }
            isLoading={ configQuery.isPlaceholderData || isLoading }
          />
        )) }
      </Grid>
      { shouldShowViewAllLink && (
        <Link
          href={ route({ pathname: '/address/[hash]', query: { hash: addressHash, tab: 'widgets' } }) }
          textStyle="sm"
          ml={ 0.5 }
        >
          View all
        </Link>
      ) }
    </Flex>
  );
};

export default Address3rdPartyWidgets;
