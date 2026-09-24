import * as yup from 'yup';
import { urlTest } from '../../utils';
import { replaceQuotes } from 'configs/app/utils';
import type { ZetaChainChainsConfigEnv } from 'types/client/zetaChain';
  
const zetaChainCCTXConfigSchema: yup.ObjectSchema<ZetaChainChainsConfigEnv> = yup.object({
    chain_id: yup.number().required(),
    chain_name: yup.string().required(),
    chain_logo: yup.string(),
    instance_url: yup.string().test(urlTest),
    address_url_template: yup.string(),
    tx_url_template: yup.string(),
});
  
export const zetaChainSchema = yup
    .object()
    .shape({
        NEXT_PUBLIC_ZETACHAIN_SERVICE_API_HOST: yup.string().test(urlTest),
        NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL: yup
        .array()
        .transform(replaceQuotes)
        .json()
        .of(zetaChainCCTXConfigSchema)
        .when('NEXT_PUBLIC_ZETACHAIN_SERVICE_API_HOST', {
            is: (value: string) => Boolean(value),
            then: (schema) => schema,
            otherwise: (schema) => schema.test(
            'not-exist',
            'NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL cannot be used if NEXT_PUBLIC_ZETACHAIN_SERVICE_API_HOST is not set',
            value => value === undefined,
            ),
        }),
        NEXT_PUBLIC_ZETACHAIN_EXTERNAL_SEARCH_CONFIG: yup
        .array()
        .transform(replaceQuotes)
        .json()
        .of(
            yup.object({
            regex: yup.string().required(),
            template: yup.string().required(),
            name: yup.string().required(),
            })
        )
        .when('NEXT_PUBLIC_ZETACHAIN_SERVICE_API_HOST', {
            is: (value: string) => Boolean(value),
            then: (schema) => schema,
            otherwise: (schema) => schema.test(
            'not-exist',
            'NEXT_PUBLIC_ZETACHAIN_EXTERNAL_SEARCH_CONFIG cannot be used if NEXT_PUBLIC_ZETACHAIN_SERVICE_API_HOST is not set',
            value => value === undefined,
            ),
        }),
    })
    .test('zetachain-api-host-dependency', 'NEXT_PUBLIC_ZETACHAIN_SERVICE_API_HOST cannot be used without NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL', function(value) {
        const hasApiHost = Boolean(value?.NEXT_PUBLIC_ZETACHAIN_SERVICE_API_HOST);
        const hasChainsConfig = Boolean(value?.NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL?.length);

        if (hasApiHost && !hasChainsConfig) {
        return this.createError({ message: 'NEXT_PUBLIC_ZETACHAIN_SERVICE_API_HOST cannot be used without NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL' });
        }

        return true;
    });
  