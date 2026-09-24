import { Flex } from '@chakra-ui/react';
import React from 'react';

import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';

import RewardsDashboardCard from './RewardsDashboardCard';

type Props = {
  title: string;
  description: string | React.ReactNode;
  imageSrc: string;
  imageWidth: string;
  imageHeight: string;
  linkText: string;
  linkHref: string;
};

const RewardsDashboardInfoCard = ({ title, description, imageSrc, imageWidth, imageHeight, linkText, linkHref }: Props) => (
  <RewardsDashboardCard
    title={ title }
    description={ description }
  >
    <Flex
      flex={ 1 }
      gap={ 4 }
      pl={ 10 }
      pr={ 7 }
      py={{ base: 4, lg: 0 }}
      flexDirection={{ base: 'column', lg: 'row' }}
      justifyContent="space-between"
      alignItems="center"
    >
      <Image
        src={ imageSrc }
        alt={ title }
        w={ imageWidth }
        h={ imageHeight }
        fallback={ <Skeleton loading/> }
      />
      <Link
        external
        href={ linkHref }
        fontSize="md"
        fontWeight="500"
      >
        { linkText }
      </Link>
    </Flex>
  </RewardsDashboardCard>
);

export default RewardsDashboardInfoCard;
