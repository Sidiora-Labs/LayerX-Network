import { replaceQuotes } from 'configs/app/utils';
import { MultichainProviderConfig } from 'types/client/multichainProviderConfig';
import * as yup from 'yup';

const multichainProviderConfigSchema: yup.ObjectSchema<MultichainProviderConfig> = yup.object({
    name: yup.string().required(),
    url_template: yup.string().required(),
    logo: yup.string().required(),
    dapp_id: yup.string(),
    promo: yup.boolean(),
});

export const multichainButtonSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_MULTICHAIN_BALANCE_PROVIDER_CONFIG: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(multichainProviderConfigSchema)
  });