import { Box, Flex, chakra } from '@chakra-ui/react';
import React from 'react';

import type { EntityTag as TEntityTag } from './types';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import { Badge } from 'toolkit/chakra/badge';
import { PopoverBody, PopoverContent, PopoverRoot, PopoverTrigger } from 'toolkit/chakra/popover';

import EntityTag from './EntityTag';

interface Props {
  className?: string;
  tags: Array<TEntityTag>;
  addressHash?: string;
  isLoading?: boolean;
  noColors?: boolean;
}

const EntityTags = ({ tags, addressHash, className, isLoading, noColors }: Props) => {
  const isMobile = useIsMobile();
  const visibleNum = isMobile ? 2 : 3;

  const metaSuitesPlaceholder = config.features.metasuites.isEnabled ?
    <Box display="none" id="meta-suites__address-tag" data-ready={ !isLoading }/> :
    null;

  if (tags.length === 0) {
    return metaSuitesPlaceholder;
  }

  const tagMaxW = (() => {
    if (tags.length === 1) {
      return { base: '100%', lg: '300px' };
    }

    if (tags.length === 2) {
      return { base: 'calc((100% - 8px) / 2)', lg: '300px' };
    }
    return { base: 'calc((100% - 40px) / 2)', lg: '300px' };
  })();

  const content = (() => {
    if (tags.length > visibleNum) {
      return (
        <>
          { tags.slice(0, visibleNum).map((tag) => (
            <EntityTag
              key={ tag.slug }
              data={ tag }
              addressHash={ addressHash }
              isLoading={ isLoading }
              maxW={ tagMaxW }
              noColors={ noColors }
            />
          )) }
          { metaSuitesPlaceholder }
          <PopoverRoot>
            <PopoverTrigger>
              <Badge loading={ isLoading } cursor="pointer" as="button" _hover={{ color: 'hover' }}>
                +{ tags.length - visibleNum }
              </Badge>
            </PopoverTrigger>
            <PopoverContent maxW="300px" w="fit-content">
              <PopoverBody>
                <Flex columnGap={ 2 } rowGap={ 2 } flexWrap="wrap">
                  { tags.slice(visibleNum).map((tag) => <EntityTag key={ tag.slug } data={ tag } addressHash={ addressHash } noColors={ noColors }/>) }
                </Flex>
              </PopoverBody>
            </PopoverContent>
          </PopoverRoot>
        </>
      );
    }

    return (
      <>
        { tags.map((tag) => (
          <EntityTag
            key={ tag.slug }
            data={ tag }
            addressHash={ addressHash }
            isLoading={ isLoading }
            maxW={ tagMaxW }
            noColors={ noColors }
          />
        )) }
        { metaSuitesPlaceholder }
      </>
    );
  })();

  return (
    <Flex className={ className } columnGap={ 2 } rowGap={ 2 } flexWrap="nowrap" alignItems="center" flexGrow={ 1 } maxW="100%" overflow="hidden">
      { content }
    </Flex>
  );
};

export default React.memo(chakra(EntityTags));
