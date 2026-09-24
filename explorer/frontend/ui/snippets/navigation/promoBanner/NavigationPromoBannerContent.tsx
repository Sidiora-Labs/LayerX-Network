import { HStack, Text, Box } from '@chakra-ui/react';

import config from 'configs/app';
import { Image } from 'toolkit/chakra/image';

import useNavLinkStyleProps from '../useNavLinkStyleProps';

const promoBanner = config.UI.navigation.promoBanner;

type Props = {
  isCollapsed?: boolean;
  isHorizontalNavigation?: boolean;
};

const NavigationPromoBannerContent = ({ isCollapsed, isHorizontalNavigation }: Props) => {
  const isExpanded = isCollapsed === false;
  const navLinkStyleProps = useNavLinkStyleProps({ isCollapsed, isExpanded });

  if (!promoBanner) {
    return null;
  }

  return 'text' in promoBanner ? (
    <HStack
      { ...navLinkStyleProps.itemProps }
      minW={ isHorizontalNavigation ? 'auto' : 'full' }
      maxW={ isHorizontalNavigation ? 'auto' : 'full' }
      w={ isHorizontalNavigation ? 'auto' : '180px' }
      gap={ 2 }
      overflow="hidden"
      whiteSpace="nowrap"
      py={ isHorizontalNavigation ? 1.5 : 2 }
      px={ isHorizontalNavigation ? 1.5 : { base: 3, lg: isExpanded ? 3 : '15px', xl: isCollapsed ? '15px' : 3 } }
      bgColor={{ _light: promoBanner.bg_color.light, _dark: promoBanner.bg_color.dark }}
    >
      <Image
        src={ promoBanner.img_url }
        alt="Promo banner icon"
        boxSize={ isHorizontalNavigation ? '20px' : '30px' }
      />
      { !isHorizontalNavigation && (
        <Text
          { ...navLinkStyleProps.textProps }
          fontWeight="medium"
          color={{ _light: promoBanner.text_color.light, _dark: promoBanner.text_color.dark }}
          overflow="hidden"
        >
          { promoBanner.text }
        </Text>
      ) }
    </HStack>
  ) : (
    <Box
      position="relative"
      minH={ isHorizontalNavigation ? 'auto' : '60px' }
    >
      <Image
        src={ promoBanner.img_url.small }
        alt="Promo banner small"
        boxSize={ isHorizontalNavigation ? '32px' : '60px' }
        borderRadius={ isHorizontalNavigation ? 'sm' : 'base' }
        position={ isHorizontalNavigation ? undefined : 'absolute' }
        top={ isHorizontalNavigation ? undefined : 'calc(50% - 30px)' }
        left={ isHorizontalNavigation ? undefined : 'calc(50% - 30px)' }
        opacity={ isHorizontalNavigation ? 1 : { base: 0, lg: isExpanded ? 0 : 1, xl: isCollapsed ? 1 : 0 } }
        transitionProperty="opacity"
        transitionDuration="normal"
        transitionTimingFunction="ease"
      />
      <Image
        display={ isHorizontalNavigation ? 'none' : 'block' }
        src={ promoBanner.img_url.large }
        alt="Promo banner large"
        w="full"
        maxW={{ base: 'full', lg: '180px' }}
        borderRadius="base"
        aspectRatio={ 2 / 1 }
        opacity={{ base: 1, lg: isExpanded ? 1 : 0, xl: isCollapsed ? 0 : 1 }}
        transitionProperty="opacity"
        transitionDuration="normal"
        transitionTimingFunction="ease"
      />
    </Box>
  );
};

export default NavigationPromoBannerContent;
