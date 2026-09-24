import { Flex, Text, Grid } from '@chakra-ui/react';
import React from 'react';

import type { MarketplaceApp } from 'types/client/marketplace';

import SocialLink from './SocialLink';
import type { Props as SocialLinkProps } from './SocialLink';
import WebsiteLink from './WebsiteLink';

interface Props {
  data: MarketplaceApp | undefined;
}

const SOCIAL_LINKS: Array<Omit<SocialLinkProps, 'href'>> = [
  { field: 'github', icon: 'social/github_filled', title: 'Github' },
  { field: 'twitter', icon: 'social/twitter_filled', title: 'X (ex-Twitter)' },
  { field: 'telegram', icon: 'social/telegram_filled', title: 'Telegram' },
  { field: 'discord', icon: 'social/discord_filled', title: 'Discord' },
];

const Content = ({ data }: Props) => {
  const socialLinks: Array<SocialLinkProps> = [];
  SOCIAL_LINKS.forEach((link) => {
    const href = data?.[link.field];
    if (href) {
      if (Array.isArray(href)) {
        href.forEach((href) => socialLinks.push({ ...link, href }));
      } else {
        socialLinks.push({ ...link, href });
      }
    }
  });

  return (
    <Flex fontSize="sm" flexDir="column" rowGap={ 5 }>
      <div>
        <Text color="text.secondary" textStyle="xs">Project info</Text>
        <Text fontSize="sm" mt={ 3 }>{ data?.shortDescription }</Text>
        <WebsiteLink url={ data?.site }/>
      </div>
      { socialLinks.length > 0 && (
        <div>
          <Text color="text.secondary" textStyle="xs">Links</Text>
          <Grid templateColumns={{ base: 'repeat(2, 1fr)', lg: 'repeat(3, 1fr)' }} columnGap={ 4 } rowGap={ 3 } mt={ 3 }>
            { socialLinks.map((link, index) => <SocialLink key={ index } { ...link }/>) }
          </Grid>
        </div>
      ) }
    </Flex>
  );
};

export default Content;
