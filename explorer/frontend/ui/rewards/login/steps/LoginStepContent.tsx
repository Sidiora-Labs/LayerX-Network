import { Text, Box, Flex, Separator } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import type { ChangeEvent } from 'react';
import React from 'react';

import { useRewardsContext } from 'lib/contexts/rewards';
import * as cookies from 'lib/cookies';
import useWallet from 'lib/web3/useWallet';
import { Alert } from 'toolkit/chakra/alert';
import { Button } from 'toolkit/chakra/button';
import { Field } from 'toolkit/chakra/field';
import { Image } from 'toolkit/chakra/image';
import { Input } from 'toolkit/chakra/input';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Switch } from 'toolkit/chakra/switch';
import { apos } from 'toolkit/utils/htmlEntities';
import useProfileQuery from 'ui/snippets/auth/useProfileQuery';

type Props = {
  goNext: (isReferral: boolean, reward: string | undefined) => void;
  closeModal: () => void;
  openAuthModal: (isAuth: boolean, trySharedLogin?: boolean) => void;
};

const LoginStepContent = ({ goNext, closeModal, openAuthModal }: Props) => {
  const router = useRouter();
  const { connect, isConnected, address } = useWallet({ source: 'Merits' });
  const savedRefCode = cookies.get(cookies.NAMES.REWARDS_REFERRAL_CODE);
  const [ isRefCodeUsed, setIsRefCodeUsed ] = React.useState(Boolean(savedRefCode));
  const [ isLoading, setIsLoading ] = React.useState(false);
  const [ refCode, setRefCode ] = React.useState(savedRefCode || '');
  const [ refCodeError, setRefCodeError ] = React.useState(false);
  const { login, checkUserQuery, rewardsConfigQuery } = useRewardsContext();
  const profileQuery = useProfileQuery();

  const isAddressMismatch = React.useMemo(() =>
    Boolean(address) &&
    Boolean(profileQuery.data?.address_hash) &&
    profileQuery.data?.address_hash !== address,
  [ address, profileQuery.data ]);

  const isLoggedIntoAccountWithWallet = React.useMemo(() =>
    !profileQuery.isLoading && profileQuery.data?.address_hash,
  [ profileQuery ]);

  const isSignUp = React.useMemo(() =>
    isConnected && !isAddressMismatch && !checkUserQuery.isFetching && !checkUserQuery.data?.exists,
  [ isConnected, isAddressMismatch, checkUserQuery ]);

  const canTrySharedLogin = rewardsConfigQuery.data?.auth?.shared_siwe_login && checkUserQuery.data?.exists !== false && !isLoggedIntoAccountWithWallet;

  const handleRefCodeChange = React.useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setRefCode(event.target.value);
  }, []);

  const loginToRewardsProgram = React.useCallback(async() => {
    try {
      setRefCodeError(false);
      setIsLoading(true);
      const { isNewUser, reward, invalidRefCodeError } = await login(isSignUp && isRefCodeUsed ? refCode : '');
      if (invalidRefCodeError) {
        setRefCodeError(true);
      } else {
        if (isNewUser) {
          goNext(isRefCodeUsed, reward);
        } else {
          closeModal();
          router.push({ pathname: '/account/merits' }, undefined, { shallow: true });
        }
      }
    } catch (error) {}
    setIsLoading(false);
  }, [ login, goNext, router, closeModal, refCode, isRefCodeUsed, isSignUp ]);

  React.useEffect(() => {
    const isInvalid = isSignUp && isRefCodeUsed && refCode.length > 0 && refCode.length !== 6 && refCode.length !== 12;
    setRefCodeError(isInvalid);
  }, [ refCode, isRefCodeUsed, isSignUp ]);

  const handleButtonClick = React.useCallback(() => {
    if (canTrySharedLogin) {
      return openAuthModal(Boolean(profileQuery.data?.email), true);
    }

    if (!isConnected) {
      return connect();
    }

    if (isLoggedIntoAccountWithWallet) {
      return loginToRewardsProgram();
    }

    return openAuthModal(Boolean(profileQuery.data?.email));
  }, [ loginToRewardsProgram, openAuthModal, profileQuery, connect, isConnected, isLoggedIntoAccountWithWallet, canTrySharedLogin ]);

  const handleToggleChange = React.useCallback(() => {
    setIsRefCodeUsed((prev) => !prev);
  }, []);

  const buttonText = React.useMemo(() => {
    if (canTrySharedLogin) {
      return 'Continue with wallet';
    }

    if (!isConnected) {
      return 'Connect wallet';
    }
    if (isLoggedIntoAccountWithWallet) {
      return isSignUp ? 'Get started' : 'Continue';
    }
    return profileQuery.data?.email ? 'Add wallet to account' : 'Log in to account';
  }, [ canTrySharedLogin, isConnected, isLoggedIntoAccountWithWallet, profileQuery.data?.email, isSignUp ]);

  return (
    <>
      <Image
        src="/static/merits/merits_program.png"
        alt="Merits program"
        mb={ 3 }
        fallback={ <Skeleton loading w="full" h="120px"/> }
      />
      <Box mb={ 6 }>
        Merits are awarded for a variety of different Blockscout activities. Connect a wallet to get started.
        <Link external href="https://docs.blockscout.com/using-blockscout/merits" ml={ 1 } fontWeight="500">
          More about Blockscout Merits
        </Link>
      </Box>
      { isSignUp && isLoggedIntoAccountWithWallet && (
        <Box mb={ 6 }>
          <Separator mb={ 6 }/>
          <Flex w="full" alignItems="center" justifyContent="space-between">
            I have a referral code
            <Switch
              size="md"
              checked={ isRefCodeUsed }
              onCheckedChange={ handleToggleChange }
              aria-label="Referral code switch"
            />
          </Flex>
          { isRefCodeUsed && (
            <Field
              label="Code"
              floating
              id="referral-code"
              size="lg"
              mt={ 3 }
              invalid={ refCodeError }
              helperText="The code should be in format XXXXXX"
              errorText={ refCodeError ? 'Incorrect code or format (6 or 12 characters)' : undefined }
            >
              <Input
                value={ refCode }
                onChange={ handleRefCodeChange }
              />
            </Field>
          ) }
        </Box>
      ) }
      { isAddressMismatch && (
        <Alert status="warning" mb={ 4 }>
          Your wallet address doesn{ apos }t match the one in your Blockscout account. Please connect the correct wallet.
        </Alert>
      ) }
      <Button
        variant="solid"
        w="full"
        whiteSpace="normal"
        mb={ 4 }
        onClick={ handleButtonClick }
        loading={ isLoading || profileQuery.isLoading || checkUserQuery.isFetching }
        loadingText={ isLoading ? 'Sign message in your wallet' : undefined }
        disabled={ isAddressMismatch || refCodeError }
      >
        { buttonText }
      </Button>
      <Text textStyle="sm" color="text.secondary" textAlign="center">
        Already registered for Blockscout Merits on another network or chain? Connect the same wallet here.
      </Text>
    </>
  );
};

export default LoginStepContent;
