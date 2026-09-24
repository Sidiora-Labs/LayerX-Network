/* eslint-disable max-len */
import { Box, Flex, List, chakra } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs-routes';

import { Button } from 'toolkit/chakra/button';
import { Link } from 'toolkit/chakra/link';
import Puzzle15 from 'ui/games/Puzzle15';
import IconSvg from 'ui/shared/IconSvg';

import AppErrorTitle from '../AppErrorTitle';
const AppErrorTxNotFound = () => {
  const snippet = {
    borderColor: { _light: 'blackAlpha.300', _dark: 'whiteAlpha.300' },
    iconBg: { _light: 'blackAlpha.800', _dark: 'whiteAlpha.800' },
    iconColor: { _light: 'white', _dark: 'black' },
  };

  const [ isPuzzleOpen, setIsPuzzleOpen ] = React.useState(false);

  const showPuzzle = React.useCallback(() => {
    setIsPuzzleOpen(true);
  }, []);

  return (
    <>
      <Box p={ 4 } borderColor={ snippet.borderColor } borderRadius="md" w="230px" borderWidth="1px">
        <Flex alignItems="center" pb={ 4 } borderBottomWidth="1px" borderColor={ snippet.borderColor }>
          { /* FIXME use non-navigation icon */ }
          <IconSvg name="navigation/transactions" boxSize={ 8 } color={ snippet.iconColor } bgColor={ snippet.iconBg } p={ 1 } borderRadius="md"/>
          <Box ml={ 2 }>
            <Box w="125px" h="8px" borderRadius="full" bgColor={ snippet.iconBg }/>
            <Box w="30px" h="8px" borderRadius="full" bgColor={ snippet.borderColor } mt={ 1.5 }/>
          </Box>
        </Flex>
        <Flex justifyContent="space-between" alignItems="center" mt={ 3 }>
          <Flex alignItems="center">
            <Box boxSize={ 5 } borderRadius="full" bgColor={ snippet.borderColor }/>
            <Box w="65px" h="8px" borderRadius="full" bgColor={ snippet.borderColor } ml={ 1.5 }/>
          </Flex>
          <Flex alignItems="center">
            <Box boxSize={ 5 } borderRadius="full" bgColor={ snippet.borderColor }/>
            <Box w="65px" h="8px" borderRadius="full" bgColor={ snippet.borderColor } ml={ 1.5 }/>
          </Flex>
        </Flex>
      </Box>
      <AppErrorTitle title="Sorry, we are unable to locate this transaction hash"/>
      <List.Root mt={ 3 } gap={ 3 } as="ol" pl={ 5 }>
        <List.Item>
          If you have just submitted this transaction please wait for at least 30 seconds before refreshing this page.
        </List.Item>
        <List.Item>
          It could still be in the TX Pool of a different node, waiting to be broadcasted.
        </List.Item>
        <List.Item>
          During times when the network is busy (i.e during ICOs) it can take a while for your transaction to propagate through the network and for us to index it.
        </List.Item>
        <List.Item>
          <span>If it still does not show up after 1 hour, please check with your </span>
          <chakra.span fontWeight={ 600 }>sender/exchange/wallet/transaction provider</chakra.span>
          <span> for additional information.</span>
        </List.Item>
        <List.Item>
          <span>If you don’t want to look for a txn and just want to have fun, <Link onClick={ showPuzzle }>solve the puzzle</Link>, and be rewarded with a secret prize.</span>
        </List.Item>
      </List.Root>
      { isPuzzleOpen && <Puzzle15/> }
      <Link href={ route({ pathname: '/' }) } asChild>
        <Button
          mt={ 8 }
          variant="outline"
        >
          Back to home
        </Button>
      </Link>
    </>
  );
};

export default AppErrorTxNotFound;
