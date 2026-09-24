import * as yup from 'yup';
import type { DeFiDropdownItem } from 'types/client/deFiDropdown';
import type { IconName } from 'public/icons/name';
import { urlTest } from '../../utils';
import { replaceQuotes } from 'configs/app/utils';

const deFiDropdownItemSchema: yup.ObjectSchema<DeFiDropdownItem> = yup
  .object({
    text: yup.string().required(),
    icon: yup.string<IconName>(),
    dappId: yup.string(),
    isEssentialDapp: yup.boolean(),
    url: yup.string().test(urlTest),
  })
  .test('oneOfRequired', 'NEXT_PUBLIC_DEFI_DROPDOWN_ITEMS: Either dappId or url is required', function(value) {
    return Boolean(value.dappId) || Boolean(value.url);
  }) as yup.ObjectSchema<DeFiDropdownItem>;

export const defiDropdownSchema = yup.object({
    NEXT_PUBLIC_DEFI_DROPDOWN_ITEMS: yup
    .array()
    .transform(replaceQuotes)
    .json()
    .of(deFiDropdownItemSchema),
});
