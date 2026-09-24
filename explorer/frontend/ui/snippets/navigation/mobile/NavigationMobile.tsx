import { Box, Flex, Text, VStack } from '@chakra-ui/react';
import React, { useCallback } from 'react';

import useNavItems, { isGroupItem } from 'lib/hooks/useNavItems';
import { useColorModeValue } from 'toolkit/chakra/color-mode';
import IconSvg from 'ui/shared/IconSvg';
import useIsAuth from 'ui/snippets/auth/useIsAuth';

import NavigationPromoBanner from '../promoBanner/NavigationPromoBanner';
import NavLink from '../vertical/NavLink';
import NavLinkRewards from '../vertical/NavLinkRewards';
import NavLinkGroup from './NavLinkGroup';

const ANIMATION_DURATION = 300;

interface Props {
  onNavLinkClick?: () => void;
  isMarketplaceAppPage?: boolean;
}

const NavigationMobile = ({ onNavLinkClick, isMarketplaceAppPage }: Props) => {
  const timeoutRef = React.useRef<number | null>(null);
  const { mainNavItems, accountNavItems } = useNavItems();

  const [ openedGroupIndex, setOpenedGroupIndex ] = React.useState(-1);
  const [ isOpen, setIsOpen ] = React.useState(false);

  const onGroupItemOpen = (index: number) => () => {
    setOpenedGroupIndex(index);
    timeoutRef.current = window.setTimeout(() => {
      setIsOpen(true);
    }, 100);
  };

  const onGroupItemClose = useCallback(() => {
    setIsOpen(false);
    timeoutRef.current = window.setTimeout(() => {
      setOpenedGroupIndex(-1);
    }, ANIMATION_DURATION);
  }, []);

  React.useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, [ ]);

  const isAuth = useIsAuth();

  const iconColor = useColorModeValue('blue.600', 'blue.300');

  const openedItem = mainNavItems[openedGroupIndex];

  const isCollapsed = isMarketplaceAppPage ? false : undefined;

  return (
    <Flex position="relative" flexDirection="column" flexGrow={ 1 }>
      <Box
        display="flex"
        flexDirection="column"
        flexGrow={ 1 }
        transform={ isOpen ? 'translateX(calc(-100% - 24px))' : 'translateX(0)' }
        transition={ `transform ${ ANIMATION_DURATION }ms ease-in-out` }
        maxHeight={ openedGroupIndex > -1 ? '100vh' : 'unset' }
        overflowY={ openedGroupIndex > -1 ? 'hidden' : 'unset' }
      >
        <Box
          as="nav"
          mt={ 6 }
        >
          <VStack
            w="100%"
            as="ul"
            gap="1"
            alignItems="flex-start"
          >
            { mainNavItems.map((item, index) => {
              if (isGroupItem(item)) {
                return <NavLinkGroup key={ item.text } item={ item } onClick={ onGroupItemOpen(index) } isExpanded={ isMarketplaceAppPage }/>;
              } else {
                return <NavLink key={ item.text } item={ item } onClick={ onNavLinkClick } isCollapsed={ isCollapsed }/>;
              }
            }) }
          </VStack>
        </Box>
        { isAuth && (
          <Box
            as="nav"
            mt={ 3 }
            pt={ 3 }
            borderTopWidth="1px"
            borderColor="border.divider"
          >
            <VStack as="ul" gap="1" alignItems="flex-start">
              <NavLinkRewards onClick={ onNavLinkClick } isCollapsed={ isCollapsed }/>
              { accountNavItems.map((item) => <NavLink key={ item.text } item={ item } onClick={ onNavLinkClick } isCollapsed={ isCollapsed }/>) }
            </VStack>
          </Box>
        ) }
        <NavigationPromoBanner isCollapsed={ isCollapsed }/>
      </Box>
      <Box
        key="sub"
        w="100%"
        mt={ 6 }
        position="absolute"
        top={ 0 }
        left={ isOpen ? 0 : 'calc(100% + 24px)' }
        transition={ `left ${ ANIMATION_DURATION }ms ease-in-out` }
      >
        <Flex alignItems="center" px={ 2 } py={ 2.5 } w="100%" h="50px" onClick={ onGroupItemClose } mb={ 1 }>
          <IconSvg name="arrows/east-mini" boxSize={ 6 } mr={ 2 } color={ iconColor }/>
          <Text color="text.secondary" fontSize="sm">{ mainNavItems[openedGroupIndex]?.text }</Text>
        </Flex>
        <Box
          w="100%"
          as="ul"
        >
          { openedItem && isGroupItem(openedItem) && openedItem.subItems?.map(
            (item, index) => Array.isArray(item) ? (
              <Box
                key={ index }
                w="100%"
                as="ul"
                _notLast={{
                  mb: 2,
                  pb: 2,
                  borderBottomWidth: '1px',
                  borderColor: 'border.divider',
                }}
              >
                { item.map(subItem => <NavLink key={ subItem.text } item={ subItem } onClick={ onNavLinkClick } isCollapsed={ isCollapsed }/>) }
              </Box>
            ) : (
              <Box key={ item.text } mb={ 1 }>
                <NavLink item={ item } onClick={ onNavLinkClick } isCollapsed={ isCollapsed }/>
              </Box>
            ),
          ) }
        </Box>
      </Box>
    </Flex>
  );
};

export default NavigationMobile;
