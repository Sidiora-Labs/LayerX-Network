import { Box, Flex, Grid, GridItem } from '@chakra-ui/react';
import React from 'react';

import type { FormSubmitResultGrouped } from '../types';

import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import EntityTag from 'ui/shared/EntityTags/EntityTag';

interface Props {
  data: FormSubmitResultGrouped;
}

const PublicTagsSubmitResultSuccess = ({ data }: Props) => {
  return (
    <Grid gridTemplateColumns={{ base: '1fr', lg: '1fr 1fr' }} rowGap={ 3 } columnGap={ 3 }>
      <GridItem overflow="hidden">
        <Box fontSize="sm" color="text.secondary" fontWeight={ 500 }>Smart contract / Address (0x...)</Box>
        <Flex flexDir="column" rowGap={ 2 } mt={ 2 }>
          { data.items
            .map(({ addresses }) => addresses)
            .flat()
            .map((hash) => (
              <AddressEntity
                key={ hash }
                address={{ hash }}
                noIcon
              />
            )) }
        </Flex>
      </GridItem>
      <GridItem>
        <Box fontSize="sm" color="text.secondary" fontWeight={ 500 }>Tag</Box>
        <Flex rowGap={ 2 } columnGap={ 2 } mt={ 2 } justifyContent="flex-start" flexWrap="wrap">
          { data.items
            .map(({ tags }) => tags)
            .flat()
            .map((tag) => (
              <EntityTag
                key={ tag.name }
                maxW={{ base: '100%', lg: '300px' }}
                data={{
                  ...tag,
                  slug: '',
                  ordinal: 0,
                }}
                noLink
              />
            )) }
        </Flex>
      </GridItem>
    </Grid>
  );
};

export default React.memo(PublicTagsSubmitResultSuccess);
