import {
  Text,
  chakra,
  Box,
  VStack,
} from '@chakra-ui/react';
import React from 'react';

import useIsMobile from 'lib/hooks/useIsMobile';
import { Link } from 'toolkit/chakra/link';
import { Tooltip } from 'toolkit/chakra/tooltip';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  className?: string;
  links?: Array<{ title: string; url: string }>;
}

const MarketplaceAppGraphLinks = ({ className, links }: Props) => {
  const isMobile = useIsMobile();

  const handleButtonClick = React.useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    e.stopPropagation();
  }, []);

  if (!links || links.length === 0) {
    return null;
  }

  const content = (
    <VStack gap={ 4 } align="start" textStyle="sm" w="260px">
      <Text>{ `This dapp uses ${ links.length > 1 ? 'several subgraphs' : 'a subgraph' } powered by The Graph` }</Text>
      { links.map(link => (
        <Link external key={ link.url } href={ link.url }>{ link.title }</Link>
      )) }
    </VStack>
  );

  return (
    <Box position="relative" className={ className } display="inline-flex" alignItems="center" onClick={ handleButtonClick }>
      <Tooltip
        variant="popover"
        content={ content }
        positioning={{ placement: isMobile ? 'bottom-end' : 'bottom' }}
        interactive
      >
        <IconSvg name="brands/graph" boxSize={ 5 } onClick={ handleButtonClick }/>
      </Tooltip>
    </Box>
  );
};

export default React.memo(chakra(MarketplaceAppGraphLinks));
