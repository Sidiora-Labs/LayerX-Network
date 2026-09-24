import { Center } from '@chakra-ui/react';
import React from 'react';

import { useColorModeValue } from 'toolkit/chakra/color-mode';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import type { IconName } from 'ui/shared/IconSvg';
import IconSvg from 'ui/shared/IconSvg';

type Props = {
  icon?: IconName;
  iconSize?: string;
  iconUrl?: Array<string>;
  text: string;
  url: string;
  isLoading?: boolean;
};

const FooterLinkItemIconExternal = ({ iconUrl, text }: { iconUrl: Array<string>; text: string }) => {
  const [ lightIconUrl, darkIconUrl ] = iconUrl;

  const imageSrc = useColorModeValue(lightIconUrl, darkIconUrl || lightIconUrl);

  return (
    <Image
      src={ imageSrc }
      alt={ `${ text } icon` }
      boxSize={ 5 }
      objectFit="contain"
    />
  );
};

const FooterLinkItem = ({ icon, iconSize, iconUrl, text, url, isLoading }: Props) => {
  if (isLoading) {
    return <Skeleton loading my="3px">{ text }</Skeleton>;
  }

  const iconElement = (() => {
    if (iconUrl && Array.isArray(iconUrl)) {
      const [ lightIconUrl, darkIconUrl ] = iconUrl;

      if (typeof lightIconUrl === 'string' && (typeof darkIconUrl === 'string' || !darkIconUrl)) {
        return <FooterLinkItemIconExternal iconUrl={ iconUrl } text={ text }/>;
      }
    }

    if (icon) {
      return (
        <Center minW={ 6 }>
          <IconSvg boxSize={ iconSize || 5 } name={ icon }/>
        </Center>
      );
    }

    return null;
  })();

  return (
    <Link href={ url } display="flex" alignItems="center" h="30px" variant="subtle" external noIcon textStyle="xs" columnGap={ 2 }>
      { iconElement }
      { text }
    </Link>
  );
};

export default FooterLinkItem;
