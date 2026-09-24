import React from 'react';
import { useSignMessage, useSwitchChain } from 'wagmi';

import type * as rewards from '@blockscout/points-types';
import type { UserInfo } from 'types/api/account';

import config from 'configs/app';
import useApiFetch from 'lib/api/useApiFetch';
import * as cookies from 'lib/cookies';
import getErrorMessage from 'lib/errors/getErrorMessage';
import getErrorObj from 'lib/errors/getErrorObj';
import getErrorObjPayload from 'lib/errors/getErrorObjPayload';
import type * as mixpanel from 'lib/mixpanel';
import useWalletReown from 'lib/web3/wallet/useWalletReown';
import { toaster } from 'toolkit/chakra/toaster';
import { YEAR } from 'toolkit/utils/consts';

function composeMessage(address: string, nonceBlockscout: string, nonceRewards: string) {
  const urlObj = window.location.hostname === 'localhost' && config.apis.rewards ?
    new URL(config.apis.rewards.endpoint) :
    window.location;

  return [
    `${ urlObj.hostname } wants you to sign in with your Ethereum account:`,
    address,
    '',
    `Sign in/up to Blockscout Account V2 & Blockscout Merits program. Merits nonce: ${ nonceRewards }.`,
    '',
    `URI: ${ urlObj.origin }`,
    'Version: 1',
    `Chain ID: ${ config.chain.id }`,
    `Nonce: ${ nonceBlockscout }`,
    `Issued At: ${ new Date().toISOString() }`,
    `Expiration Time: ${ new Date(Date.now() + YEAR).toISOString() }`,
  ].join('\n');
}

interface Props {
  onSuccess?: ({ address, profile, rewardsToken }: { address: string; profile: UserInfo; rewardsToken?: string }) => void;
  onError?: () => void;
  source?: mixpanel.EventPayload<mixpanel.EventTypes.WALLET_CONNECT>['Source'];
  isAuth?: boolean;
  loginToRewards?: boolean;
  fetchProtectedResource: <T>(fetcher: (token?: string) => Promise<T>, token?: string) => Promise<T>;
}

function useSignInWithWallet({ onSuccess, onError, source = 'Login', isAuth, loginToRewards, fetchProtectedResource }: Props) {
  const [ isPending, setIsPending ] = React.useState(false);
  const isConnectingWalletRef = React.useRef(false);

  const apiFetch = useApiFetch();
  const web3Wallet = useWalletReown({ source });
  const { signMessageAsync } = useSignMessage();
  const { switchChainAsync } = useSwitchChain();

  const getSiweMessage = React.useCallback(async(address: string) => {
    try {
      if (!loginToRewards) {
        throw new Error('Login to rewards is not enabled');
      }

      if (cookies.get(cookies.NAMES.REWARDS_API_TOKEN)) {
        throw new Error('User already has logged in to rewards');
      }

      const rewardsConfig = await apiFetch('rewards:config') as rewards.GetConfigResponse;
      if (!rewardsConfig.auth?.shared_siwe_login) {
        throw new Error('Shared SIWE login is not enabled');
      }

      const rewardsCheckUser = await apiFetch('rewards:check_user', { pathParams: { address } }) as rewards.AuthUserResponse;
      if (!rewardsCheckUser.exists) {
        throw new Error('Rewards user does not exist');
      }

      const nonceConfig = await apiFetch(
        'rewards:nonce',
        { queryParams: { blockscout_login_address: address, blockscout_login_chain_id: config.chain.id } },
      ) as rewards.AuthNonceResponse;
      if (!nonceConfig.merits_login_nonce || !nonceConfig.nonce) {
        throw new Error('Cannot get merits login nonce');
      }

      return {
        message: composeMessage(address, nonceConfig.nonce, nonceConfig.merits_login_nonce),
        authNonce: nonceConfig.nonce,
        rewardsNonce: nonceConfig.merits_login_nonce,
        type: 'shared',
      };
    } catch (error) {
      const response = await apiFetch('general:auth_siwe_message', { queryParams: { address } }) as { siwe_message: string };
      return {
        message: response.siwe_message,
        type: 'single',
      };
    }
  }, [ apiFetch, loginToRewards ]);

  const authFetchFactory = React.useCallback((message: string, signature: string) => (recaptchaToken?: string) => {
    const authResource = isAuth ? 'general:auth_link_address' : 'general:auth_siwe_verify';
    return apiFetch<typeof authResource, UserInfo, unknown>(authResource, {
      fetchParams: {
        method: 'POST',
        body: { message, signature, recaptcha_response: recaptchaToken },
        headers: {
          ...(recaptchaToken && { 'recaptcha-v2-response': recaptchaToken }),
        },
      },
    });
  }, [ apiFetch, isAuth ]);

  const proceedToAuth = React.useCallback(async(address: string) => {
    try {
      await switchChainAsync({ chainId: Number(config.chain.id) });
      const siweMessage = await getSiweMessage(address);
      const signature = await signMessageAsync({ message: siweMessage.message });

      const authResponse = await fetchProtectedResource(authFetchFactory(siweMessage.message, signature));

      const rewardsLoginResponse = siweMessage.type === 'shared' ?
        await apiFetch('rewards:login', {
          fetchParams: {
            method: 'POST',
            body: {
              nonce: siweMessage.authNonce,
              message: siweMessage.message,
              signature,
            },
          },
        }) as rewards.AuthLoginResponse : undefined;

      if (!('name' in authResponse)) {
        throw Error('Something went wrong');
      }
      onSuccess?.({ address, profile: authResponse, rewardsToken: rewardsLoginResponse?.token });
    } catch (error) {
      const errorObj = getErrorObj(error);
      const apiErrorMessage = getErrorObjPayload<{ message: string }>(error)?.message;
      const shortMessage = errorObj && 'shortMessage' in errorObj && typeof errorObj.shortMessage === 'string' ? errorObj.shortMessage : undefined;
      onError?.();
      toaster.error({
        title: 'Error',
        description: apiErrorMessage || shortMessage || getErrorMessage(error) || 'Something went wrong',
      });
    } finally {
      setIsPending(false);
    }
  }, [ switchChainAsync, getSiweMessage, signMessageAsync, fetchProtectedResource, authFetchFactory, apiFetch, onSuccess, onError ]);

  const start = React.useCallback(() => {
    setIsPending(true);
    if (web3Wallet.address) {
      proceedToAuth(web3Wallet.address);
    } else {
      isConnectingWalletRef.current = true;
      web3Wallet.openModal();
    }
  }, [ proceedToAuth, web3Wallet ]);

  React.useEffect(() => {
    if (web3Wallet.address && isConnectingWalletRef.current) {
      isConnectingWalletRef.current = false;
      proceedToAuth(web3Wallet.address);
    }
  }, [ proceedToAuth, web3Wallet.address ]);

  return React.useMemo(() => ({ start, isPending }), [ start, isPending ]);
}

function useSignInWithWalletFallback() {
  return React.useMemo(() => ({ start: () => {}, isPending: false }), [ ]);
}

export default config.features.blockchainInteraction.isEnabled ? useSignInWithWallet : useSignInWithWalletFallback;
