import { replaceQuotes } from "configs/app/utils";
import { Address3rdPartyWidget, ADDRESS_3RD_PARTY_WIDGET_PAGES } from "types/views/address";
import * as yup from 'yup';

export const address3rdPartyWidgetsConfigSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS_CONFIG_URL: yup
      .mixed()
      .test('shape', 'Invalid schema were provided for NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS_CONFIG_URL, it should have name, url, icon, title, value', (data) => {
        const isUndefined = data === undefined;
        const parsedData = typeof data === 'string' ? JSON.parse(data) : data;
        const valueSchema = yup.lazy((objValue) => {
          let schema = yup.object();
          Object.keys(objValue).forEach((key) => {
            schema = schema.shape({
              [key]: yup.object<Address3rdPartyWidget>().shape({
                name: yup.string().required(),
                url: yup.string().required(),
                icon: yup.string().required(),
                title: yup.string().required(),
                hint: yup.string().optional(),
                valuePath: yup.string().required(),
                pages: yup.array().of(yup.string().oneOf(ADDRESS_3RD_PARTY_WIDGET_PAGES)).required(),
                chainIds: yup.object<Record<string, string>>().optional(),
              }),
            });
          });
          return schema;
        });
        return isUndefined || valueSchema.isValidSync(parsedData);
      }),
    NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(yup.string())
      .when('NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS_CONFIG_URL', {
        is: (value: string) => value,
        then: (schema) => schema,
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS cannot not be used if NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS_CONFIG_URL is not provided'),
      }),
  });   