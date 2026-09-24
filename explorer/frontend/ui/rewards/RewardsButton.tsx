import { chakra } from '@chakra-ui/react';
import React, { useCallback } from 'react';

import { route } from 'nextjs-routes';

import { useRewardsContext } from 'lib/contexts/rewards';
import useIsMobile from 'lib/hooks/useIsMobile';
import type { ButtonProps } from 'toolkit/chakra/button';
import { Button } from 'toolkit/chakra/button';
import { Tooltip } from 'toolkit/chakra/tooltip';
import IconSvg from 'ui/shared/IconSvg';

type Props = {
  size?: ButtonProps['size'];
  variant?: ButtonProps['variant'];
};

const RewardsButton = ({ variant = 'header', size }: Props) => {
  const { isInitialized, isAuth, openLoginModal, dailyRewardQuery, balancesQuery } = useRewardsContext();
  const isMobile = useIsMobile();
  const isLoading = !isInitialized || dailyRewardQuery.isLoading || balancesQuery.isLoading;

  const handleFocus = useCallback((e: React.FocusEvent<HTMLButtonElement>) => {
    e.preventDefault();
  }, []);

  return (
    <Tooltip
      content="Earn Merits for using Blockscout"
      openDelay={ 500 }
      disabled={ isMobile || isLoading || isAuth }
    >
      <Button
        variant={ variant }
        selected={ !isLoading && isAuth }
        flexShrink={ 0 }
        as={ isAuth ? 'a' : 'button' }
        { ...(isAuth ? { href: route({ pathname: '/account/merits' }) } : {}) }
        onClick={ isAuth ? undefined : openLoginModal }
        onFocus={ handleFocus }
        size={ size }
        px={{ base: '10px', lg: 3 }}
        loading={ isLoading }
        _hover={{
          textDecoration: 'none',
        }}
      >
        <IconSvg
          name={ dailyRewardQuery.data?.available ? 'merits_with_dot' : 'merits' }
          boxSize={ variant === 'hero' ? 6 : 5 }
          flexShrink={ 0 }
        />
        <chakra.span
          display={{ base: 'none', md: 'inline' }}
          fontWeight={ isAuth ? '700' : '600' }
        >
          { isAuth ? (balancesQuery.data?.total || 'N/A') : 'Merits' }
        </chakra.span>
      </Button>
    </Tooltip>
  );
};

export default RewardsButton;
