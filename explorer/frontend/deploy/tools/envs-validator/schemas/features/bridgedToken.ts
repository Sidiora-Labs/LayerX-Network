import * as yup from 'yup';
import { urlTest } from '../../utils';
import { replaceQuotes } from 'configs/app/utils';
import type { BridgedTokenChain, TokenBridge } from 'types/client/token';

const bridgedTokenChainSchema: yup.ObjectSchema<BridgedTokenChain> = yup
  .object({
    id: yup.string().required(),
    title: yup.string().required(),
    short_title: yup.string().required(),
    base_url: yup.string().test(urlTest).required(),
  });

const tokenBridgeSchema: yup.ObjectSchema<TokenBridge> = yup
  .object({
    type: yup.string().required(),
    title: yup.string().required(),
    short_title: yup.string().required(),
  });

export const bridgedTokensSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_BRIDGED_TOKENS_CHAINS: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(bridgedTokenChainSchema),
    NEXT_PUBLIC_BRIDGED_TOKENS_BRIDGES: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(tokenBridgeSchema)
      .when('NEXT_PUBLIC_BRIDGED_TOKENS_CHAINS', {
        is: (value: Array<unknown>) => value && value.length > 0,
        then: (schema) => schema.required(),
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_BRIDGED_TOKENS_BRIDGES cannot not be used without NEXT_PUBLIC_BRIDGED_TOKENS_CHAINS'),
      }),
  });