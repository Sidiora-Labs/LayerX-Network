import { Box, Flex, Grid, GridItem } from '@chakra-ui/react';
import { pickBy } from 'es-toolkit';
import React from 'react';

import type { FormSubmitResultGrouped } from '../types';

import { route } from 'nextjs-routes';

import useIsMobile from 'lib/hooks/useIsMobile';
import { Button } from 'toolkit/chakra/button';
import { Link } from 'toolkit/chakra/link';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import EntityTag from 'ui/shared/EntityTags/EntityTag';
interface Props {
  data: FormSubmitResultGrouped;
}

const PublicTagsSubmitResultWithErrors = ({ data }: Props) => {
  const isMobile = useIsMobile();
  const bgColorSuccess = { _light: 'green.50', _dark: 'green.800' };
  const bgColorError = { _light: 'red.50', _dark: 'red.800' };

  return (
    <Flex flexDir="column" rowGap={ 3 }>
      { data.items.map((item, index) => {

        const startOverButtonQuery = pickBy({
          addresses: item.addresses,
          requesterName: data.requesterName,
          requesterEmail: data.requesterEmail,
          companyName: data.companyName,
          companyWebsite: data.companyWebsite,
        }, Boolean);

        return (
          <Flex key={ index } flexDir={{ base: 'column', lg: 'row' }}>
            <Box flexGrow={ 1 }>
              <Grid
                gridTemplateColumns={{ base: '1fr', lg: '1fr 1fr' }}
                bgColor={ item.error ? bgColorError : bgColorSuccess }
                borderRadius="base"
                rowGap={ 3 }
              >
                <GridItem px={{ base: 4, lg: 6 }} pt={{ base: 2, lg: 4 }} pb={{ base: 0, lg: 4 }} overflow="hidden">
                  <Box fontSize="sm" color="text.secondary" fontWeight={ 500 }>Smart contract / Address (0x...)</Box>
                  <Flex flexDir="column" rowGap={ 2 } mt={ 2 }>
                    { item.addresses.map((hash) => (
                      <AddressEntity
                        key={ hash }
                        address={{ hash }}
                        noIcon
                      />
                    )) }
                  </Flex>
                </GridItem>
                <GridItem px={{ base: 4, lg: 6 }} pb={{ base: 2, lg: 4 }} pt={{ base: 0, lg: 4 }}>
                  <Box fontSize="sm" color="text.secondary" fontWeight={ 500 }>Tag</Box>
                  <Flex rowGap={ 2 } columnGap={ 2 } mt={ 2 } justifyContent="flex-start" flexWrap="wrap">
                    { item.tags.map((tag) => (
                      <EntityTag
                        key={ tag.name }
                        maxW={{ base: '100%', lg: '300px' }}
                        data={{ ...tag, slug: '', ordinal: 0 }}
                        noLink
                      />
                    )) }
                  </Flex>
                </GridItem>
              </Grid>
              { item.error && <Box color="red.500" mt={ 1 } fontSize="sm">{ item.error }</Box> }
            </Box>
            { item.error && (
              <Link
                href={ route({ pathname: '/public-tags/submit', query: startOverButtonQuery }) }
                asChild
              >
                <Button
                  variant="outline"
                  size="sm"
                  flexShrink={ 0 }
                  mt={{ base: 1, lg: 6 }}
                  ml={{ base: 0, lg: 6 }}
                  w="min-content"
                >
                  Start  over
                </Button>
              </Link>
            ) }
            { !item.error && !isMobile && <Box w="95px" ml={ 6 } flexShrink={ 0 }/> }
          </Flex>
        );
      }) }
    </Flex>
  );
};

export default React.memo(PublicTagsSubmitResultWithErrors);
