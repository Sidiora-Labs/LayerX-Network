import { Grid } from '@chakra-ui/react';

import config from 'configs/app';
import { apos } from 'toolkit/utils/htmlEntities';

import RewardsDashboardInfoCard from '../RewardsDashboardInfoCard';

export default function ResourcesTab() {
  return (
    <Grid
      w="full"
      gap={ 6 }
      templateColumns={{ base: '1fr', md: 'repeat(2, 1fr)' }}
    >
      <RewardsDashboardInfoCard
        title="Badges"
        description={ `Collect limited and legendary badges by completing different Blockscout related tasks.
          Go to the badges website to see what${ apos }s available and start your collection today.` }
        imageSrc="/static/merits/badges.svg"
        imageWidth="180px"
        imageHeight="86px"
        linkText="View badges"
        linkHref={ `https://merits.blockscout.com/?tab=badges&utm_source=${ config.chain.id }&utm_medium=badges` }
      />
      <RewardsDashboardInfoCard
        title="Blockscout campaigns"
        description="Join Blockscout activities to earn bonus Merits and exclusive rewards from our partners!"
        imageSrc="/static/merits/campaigns.svg"
        imageWidth="180px"
        imageHeight="76px"
        linkText="Check campaigns"
        linkHref={ `https://merits.blockscout.com/?tab=campaigns&utm_source=${ config.chain.id }&utm_medium=campaigns` }
      />
      <RewardsDashboardInfoCard
        title="Use your Merits"
        description="Spend your Merits to get exclusive discounts and offers across several web3 products!"
        imageSrc="/static/merits/offers.svg"
        imageWidth="180px"
        imageHeight="86px"
        linkText="Check offers"
        linkHref={ `https://merits.blockscout.com/?tab=spend&utm_source=${ config.chain.id }&utm_medium=spend` }
      />
    </Grid>
  );
}
