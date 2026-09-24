import type { HTMLChakraProps } from '@chakra-ui/react';
import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { PaginationParams } from './types';

import { Button } from 'toolkit/chakra/button';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Skeleton } from 'toolkit/chakra/skeleton';
import IconSvg from 'ui/shared/IconSvg';

interface Props extends PaginationParams, Omit<HTMLChakraProps<'div'>, 'page' | 'direction'> {}

const Pagination = (props: Props) => {
  const { page, onNextPageClick, onPrevPageClick, resetPage, hasPages, hasNextPage, canGoBackwards, isLoading, isVisible, ...rest } = props;

  if (!isVisible) {
    return null;
  }

  const showSkeleton = page === 1 && !hasPages && isLoading;

  return (
    <Flex
      as="nav"
      alignItems="center"
      { ...rest }
    >
      <Skeleton loading={ showSkeleton } mr={ 3 }>
        <Button
          variant="pagination"
          size="sm"
          onClick={ resetPage }
          disabled={ page === 1 || isLoading }
        >
          First
        </Button>
      </Skeleton>
      <IconButton
        aria-label="Prev page"
        variant="pagination"
        boxSize={ 8 }
        onClick={ onPrevPageClick }
        disabled={ !canGoBackwards || isLoading || page === 1 }
        loadingSkeleton={ showSkeleton }
      >
        <IconSvg name="arrows/east-mini" boxSize={ 5 }/>
      </IconButton>
      <Button
        variant="pagination"
        size="sm"
        selected={ !showSkeleton }
        pointerEvents="none"
        loadingSkeleton={ showSkeleton }
        mx={ 2 }
        minW={ 8 }
        px={ 2 }
      >
        { page }
      </Button>
      <IconButton
        aria-label="Next page"
        variant="pagination"
        boxSize={ 8 }
        onClick={ onNextPageClick }
        disabled={ !hasNextPage || isLoading }
        loadingSkeleton={ showSkeleton }
      >
        <IconSvg name="arrows/east-mini" boxSize={ 5 } transform="rotate(180deg)"/>
      </IconButton>
    </Flex>

  );
};

export default React.memo(Pagination);
