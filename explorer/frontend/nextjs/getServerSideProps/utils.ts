import type { GetServerSidePropsContext, GetServerSidePropsResult } from 'next';

import type { Route } from 'nextjs-routes';

import config from 'configs/app';
import multichainConfig from 'configs/multichain';

import type { Guard } from './guards';
import * as handlers from './handlers';
import type { Props } from './handlers';

export const factory = (guards: Array<Guard | Array<Guard>>, chainConfig = config) => {
  return async<Pathname extends Route['pathname'] = never>(context: GetServerSidePropsContext): Promise<GetServerSidePropsResult<Props<Pathname>>> => {
    for (const guard of guards) {
      if (Array.isArray(guard)) {
        // When a guard is an array, it works as OR conditional statement
        // If any of the guards returns nothing, we continue to the next guard
        // If all of the guards return something, we return the result of the first one that returns something
        const results = await Promise.all(guard.map((subGuard) => subGuard(chainConfig)(context)));
        if (results.some((result) => !result)) {
          continue;
        } else {
          return results.find((result) => result) as GetServerSidePropsResult<Props<Pathname>>;
        }
      } else {
        const result = await guard(chainConfig)(context);
        if (result) {
          return result;
        }
      }
    }
    return handlers.base<Pathname>(context);
  };
};

export const factoryMultichain = (guards: Array<Guard>) => {
  return async(context: GetServerSidePropsContext) => {
    const chainSlug = context.params?.chain_slug;
    const chain = multichainConfig()?.chains.find((chain) => chain.slug === chainSlug);

    if (!chain?.app_config) {
      return {
        notFound: true,
      };
    }

    return factory(guards, chain.app_config)(context);
  };
};
