import { chakra } from '@chakra-ui/react';
import { useQuery } from '@tanstack/react-query';
import React from 'react';

import type { HomeStats } from 'types/api/stats';

import { Image } from 'toolkit/chakra/image';
import { Skeleton } from 'toolkit/chakra/skeleton';
import TokenLogoPlaceholder from 'ui/shared/TokenLogoPlaceholder';

import useFetchParentChainApi from '../home/useFetchParentChainApi';

interface Props {
  isLoading?: boolean;
  className?: string;
};

const NativeTokenIcon = ({ isLoading, className }: Props) => {
  const parentChainApiFetch = useFetchParentChainApi();
  const parentChainStatsQuery = useQuery({
    queryKey: [ 'parent_chain', 'stats' ],
    queryFn: () => parentChainApiFetch({ path: '/stats' }) as Promise<HomeStats>,
    refetchOnMount: false,
  });

  if (isLoading || parentChainStatsQuery.isFetching) {
    return <Skeleton borderRadius="base" loading className={ className }/>;
  }

  return (
    <Image
      className={ className }
      borderRadius="base"
      src={ parentChainStatsQuery.data?.coin_image || undefined }
      alt="Native token logo"
      fallback={ <TokenLogoPlaceholder/> }
    />
  );
};

export default chakra(NativeTokenIcon);
