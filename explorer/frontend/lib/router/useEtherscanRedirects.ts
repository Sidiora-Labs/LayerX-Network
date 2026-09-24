import { useRouter } from 'next/router';
import React from 'react';

import getQueryParamString from './getQueryParamString';

export default function useEtherscanRedirects() {
  const router = useRouter();

  // The browser does not send the segment hash (or anchor) to the server,
  // so we have to handle such redirects for the etherscan-like links on the client side.
  React.useEffect(() => {
    const segmentHash = window.location.hash;

    if (segmentHash) {
      switch (router.route) {
        case '/tx/[hash]': {
          const hash = getQueryParamString(router.query.hash);

          switch (segmentHash) {
            case '#statechange':
              router.replace({
                pathname: '/tx/[hash]',
                query: { hash, tab: 'state' },
              });
              break;
            case '#eventlog':
              router.replace({
                pathname: '/tx/[hash]',
                query: { hash, tab: 'logs' },
              });
              break;
            case '#aa':
              router.replace({
                pathname: '/tx/[hash]',
                query: { hash, tab: 'user_ops' },
              });
              break;
            case '#internal':
              router.replace({
                pathname: '/tx/[hash]',
                query: { hash, tab: 'internal' },
              });
              break;
          }
          break;
        }

        case '/address/[hash]': {
          const hash = getQueryParamString(router.query.hash);

          switch (segmentHash) {
            case '#internaltx':
              router.replace({
                pathname: '/address/[hash]',
                query: { hash, tab: 'internal_txns' },
              });
              break;
            case '#tokentxns':
              router.replace({
                pathname: '/address/[hash]',
                query: { hash, tab: 'token_transfers' },
              });
              break;
            case '#asset-tokens':
              router.replace({
                pathname: '/address/[hash]',
                query: { hash, tab: 'tokens' },
              });
              break;
            case '#asset-nfts':
              router.replace({
                pathname: '/address/[hash]',
                query: { hash, tab: 'tokens_nfts' },
              });
              break;
            case '#aatx':
              router.replace({
                pathname: '/address/[hash]',
                query: { hash, tab: 'user_ops' },
              });
              break;
            case '#code':
              router.replace({
                pathname: '/address/[hash]',
                query: { hash, tab: 'contract' },
              });
              break;
            case '#readContract':
              router.replace({
                pathname: '/address/[hash]',
                query: { hash, tab: 'read_contract' },
              });
              break;
            case '#writeContract':
              router.replace({
                pathname: '/address/[hash]',
                query: { hash, tab: 'write_contract' },
              });
              break;
          }
          break;
        }

        case '/token/[hash]': {
          const hash = getQueryParamString(router.query.hash);

          switch (segmentHash) {
            case '#balances':
              router.replace({
                pathname: '/token/[hash]',
                query: { hash, tab: 'holders' },
              });
              break;
          }
          break;
        }

        case '/stats': {
          switch (segmentHash) {
            case '#section-contracts-data':
              router.replace({
                pathname: '/stats',
                hash: 'contracts',
              });
              break;
          }
          break;
        }

        default:
          break;
      }
    }
  // run only on mount
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ ]);
}
