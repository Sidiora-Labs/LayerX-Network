import { Flex, Text } from '@chakra-ui/react';
import type { MouseEvent } from 'react';
import React from 'react';

import type { MarketplaceApp } from 'types/client/marketplace';

import { useColorModeValue } from 'toolkit/chakra/color-mode';
import { Heading } from 'toolkit/chakra/heading';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Image } from 'toolkit/chakra/image';
import { Link, LinkBox } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';

import FavoriteIcon from '../FavoriteIcon';
import MarketplaceAppCardLink from '../MarketplaceAppCardLink';
import MarketplaceAppIntegrationIcon from '../MarketplaceAppIntegrationIcon';

interface Props extends MarketplaceApp {
  onInfoClick: (event: MouseEvent) => void;
  isFavorite: boolean;
  onFavoriteClick: () => void;
  isLoading: boolean;
  onAppClick: (event: MouseEvent, id: string) => void;
}

const FeaturedAppMobile = ({
  id,
  url,
  external,
  title,
  logo,
  logoDarkMode,
  shortDescription,
  categories,
  onInfoClick,
  isFavorite,
  onFavoriteClick,
  isLoading,
  internalWallet,
  onAppClick,
}: Props) => {
  const categoriesLabel = categories.join(', ');

  const logoUrl = useColorModeValue(logo, logoDarkMode || logo);

  return (
    <LinkBox
      borderRadius="md"
      padding={{ base: 3, sm: '20px' }}
      role="group"
      background={{ _light: 'purple.50', _dark: 'whiteAlpha.100' }}
    >
      <Flex
        flexDirection="row"
        height="100%"
        alignContent="start"
        gap={ 4 }
      >
        <Flex
          flexDirection="column"
          alignItems="center"
          justifyContent="space-between"
        >
          <Skeleton
            loading={ isLoading }
            w={{ base: '64px', sm: '96px' }}
            h={{ base: '64px', sm: '96px' }}
            display="flex"
            alignItems="center"
            justifyContent="center"
          >
            <Image
              src={ isLoading ? undefined : logoUrl }
              alt={ `${ title } app icon` }
              borderRadius="8px"
            />
          </Skeleton>

          { !isLoading && (
            <Flex
              position={{ base: 'relative', sm: 'absolute' }}
              right={{ base: 0, sm: '50px' }}
              top={{ base: 0, sm: '24px' }}
            >
              <Link
                fontSize={{ base: 'xs', sm: 'sm' }}
                fontWeight="500"
                paddingRight={{ sm: 2 }}
                href="#"
                onClick={ onInfoClick }
              >
                More info
              </Link>
            </Flex>
          ) }
        </Flex>

        <Flex flexDirection="column" gap={ 2 }>
          <Skeleton
            loading={ isLoading }
            paddingRight={{ base: '25px', sm: '110px' }}
            display="flex"
            alignItems="center"
          >
            <Heading level="3">
              <MarketplaceAppCardLink
                id={ id }
                url={ url }
                external={ external }
                title={ title }
                onClick={ onAppClick }
              />
            </Heading>
            <MarketplaceAppIntegrationIcon external={ external } internalWallet={ internalWallet }/>
          </Skeleton>

          <Skeleton
            loading={ isLoading }
            color="text.secondary"
            textStyle="xs"
          >
            <span>{ categoriesLabel }</span>
          </Skeleton>

          <Skeleton
            loading={ isLoading }
            asChild
          >
            <Text lineClamp={ 3 } textStyle="xs">
              { shortDescription }
            </Text>
          </Skeleton>
        </Flex>

        { !isLoading && (
          <IconButton
            display="flex"
            alignItems="center"
            justifyContent="center"
            position="absolute"
            right={{ base: 1, sm: '10px' }}
            top={{ base: 1, sm: '18px' }}
            aria-label="Mark as favorite"
            title="Mark as favorite"
            variant="icon_background"
            size="md"
            onClick={ onFavoriteClick }
            selected={ isFavorite }
          >
            <FavoriteIcon isFavorite={ isFavorite }/>
          </IconButton>
        ) }
      </Flex>
    </LinkBox>
  );
};

export default React.memo(FeaturedAppMobile);
