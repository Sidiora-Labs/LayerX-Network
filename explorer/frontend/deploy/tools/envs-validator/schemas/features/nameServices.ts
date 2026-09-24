import { urlTest } from "../../utils";
import * as yup from 'yup';
import { replaceQuotes } from 'configs/app/utils';
import type { ApiDocsTabId } from 'types/views/apiDocs';
import { API_DOCS_TABS } from 'types/views/apiDocs';

export const nameServicesSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_NAME_SERVICE_API_HOST: yup.string().test(urlTest),
    NEXT_PUBLIC_NAME_SERVICE_PROTOCOLS: yup
        .array()
        .transform(replaceQuotes)
        .json()
        .when('NEXT_PUBLIC_NAME_SERVICE_API_HOST', {
            is: (value: string) => Boolean(value),
            then: (schema) => schema.of(yup.string()).min(1).optional(),
            otherwise: (schema) => schema.test(
                'not-exist',
                'NEXT_PUBLIC_NAME_SERVICE_PROTOCOLS cannot not be used if NEXT_PUBLIC_NAME_SERVICE_API_HOST is not set',
                value => value === undefined,
            ),
        }),

    NEXT_PUBLIC_CLUSTERS_API_HOST: yup.string().test(urlTest),
    NEXT_PUBLIC_CLUSTERS_CDN_URL: yup
        .string()
        .when('NEXT_PUBLIC_CLUSTERS_API_HOST', {
            is: (value: string) => Boolean(value),
            then: (schema) => schema.test(urlTest).optional(),
            otherwise: (schema) => schema.test(
                'not-exist',
                'NEXT_PUBLIC_CLUSTERS_CDN_URL cannot not be used if NEXT_PUBLIC_CLUSTERS_API_HOST is not set',
                value => value === undefined,
            ),
        }),
  });