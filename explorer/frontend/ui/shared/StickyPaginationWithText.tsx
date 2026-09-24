import { Box } from '@chakra-ui/react';
import React from 'react';

import type { PaginationParams } from './pagination/types';

import ActionBar from './ActionBar';
import Pagination from './pagination/Pagination';

type Props = {
  pagination: PaginationParams;
  text: React.ReactNode;
};

const StickyPaginationWithText = ({ pagination, text }: Props) => {
  if (!pagination.isVisible) {
    return <Box mb={ 6 }>{ text }</Box>;
  }

  return (
    <>
      <Box mb={ 6 } display={{ base: 'block', lg: 'none' }}>
        { text }
      </Box>
      <ActionBar mt={ -6 } alignItems="center">
        <Box display={{ base: 'none', lg: 'block' }}>
          { text }
        </Box>
        { pagination.isVisible && <Pagination ml="auto" { ...pagination }/> }
      </ActionBar>
    </>
  );
};

export default StickyPaginationWithText;
