import { Box, chakra, Flex, Separator } from '@chakra-ui/react';
import React from 'react';

import type { EntityTag } from './types';

import * as mixpanel from 'lib/mixpanel/index';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { makePrettyLink } from 'toolkit/utils/url';

interface Props {
  data: EntityTag;
  children: React.ReactNode;
}

const EntityTagTooltip = ({ data, children }: Props) => {
  const hasPopover = Boolean(
    data.meta?.tooltipIcon ||
    data.meta?.tooltipTitle ||
    data.meta?.tooltipDescription ||
    data.meta?.tooltipUrl ||
    data.meta?.tooltipAttribution,
  );

  const link = makePrettyLink(data.meta?.tooltipUrl);

  const attribution = React.useMemo(() => {
    if (!data.meta?.tooltipAttribution) {
      return;
    }
    const link = makePrettyLink(data.meta?.tooltipAttribution);
    return link ?? data.meta.tooltipAttribution;
  }, [ data.meta?.tooltipAttribution ]);

  const handleLinkClick = React.useCallback(() => {
    if (!data.meta?.tooltipUrl) {
      return;
    }

    mixpanel.logEvent(mixpanel.EventTypes.PAGE_WIDGET, {
      Type: 'Address tag',
      Info: data.slug,
      URL: data.meta.tooltipUrl,
    });
  }, [ data.meta?.tooltipUrl, data.slug ]);

  if (!hasPopover) {
    // eslint-disable-next-line react/jsx-no-useless-fragment
    return <>{ children }</>;
  }

  const content = (
    <Box className="dark">
      <Flex textStyle="sm" flexDir="column" rowGap={ 2 } textAlign="left" _empty={{ display: 'none' }}>
        { (data.meta?.tooltipIcon || data.meta?.tooltipTitle) && (
          <Flex columnGap={ 3 } alignItems="center">
            { data.meta?.tooltipIcon && <Image src={ data.meta.tooltipIcon } boxSize="30px" alt={ `${ data.name } tag logo` }/> }
            { data.meta?.tooltipTitle && <chakra.span fontWeight="600">{ data.meta.tooltipTitle }</chakra.span> }
          </Flex>
        ) }
        { data.meta?.tooltipDescription && <chakra.span>{ data.meta.tooltipDescription }</chakra.span> }
        { link && <Link external href={ link.href } onClick={ handleLinkClick }>{ link.domain }</Link> }
      </Flex>
      { attribution ? (
        <>
          { (data.meta?.tooltipIcon || data.meta?.tooltipTitle || data.meta?.tooltipDescription || link) && <Separator mt={ 2 } mb={ 1 }/> }
          <Flex alignItems="center" color="text.secondary" textStyle="xs">
            <chakra.span mr={ 2 }>Source:</chakra.span>
            { data.meta?.tooltipAttributionIcon && <Image src={ data.meta.tooltipAttributionIcon } boxSize={ 4 } mr={ 1 } zIndex={ 1 }/> }
            { typeof attribution === 'string' ?
              <chakra.span fontWeight="500">{ attribution }</chakra.span> :
              <Link external href={ attribution.href }>{ attribution.domain }</Link> }
          </Flex>
        </>
      ) : null }
    </Box>
  );

  return (
    <Tooltip content={ content } interactive>
      { children }
    </Tooltip>
  );
};

export default React.memo(EntityTagTooltip);
