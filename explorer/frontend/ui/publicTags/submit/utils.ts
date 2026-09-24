import { pickBy, isEqual } from 'es-toolkit';

import type { FormFieldTag, FormFields, FormSubmitResult, FormSubmitResultGrouped, FormSubmitResultItemGrouped, SubmitRequestBody } from './types';
import type { UserInfo } from 'types/api/account';

import type { Route } from 'nextjs-routes';

import getQueryParamString from 'lib/router/getQueryParamString';

export function convertFormDataToRequestsBody(data: FormFields): Array<SubmitRequestBody> {
  const result: Array<SubmitRequestBody> = [];

  for (const address of data.addresses) {
    for (const tag of data.tags) {
      result.push({
        requesterName: data.requesterName,
        requesterEmail: data.requesterEmail,
        companyName: data.companyName,
        companyWebsite: data.companyWebsite,
        address: address.hash,
        name: tag.name,
        tagType: tag.type[0],
        description: data.description,
        meta: pickBy({
          bgColor: tag.bgColor,
          textColor: tag.textColor,
          tagUrl: tag.url,
          tagIcon: tag.iconUrl,
          tooltipDescription: tag.tooltipDescription,
        }, Boolean),
      });
    }
  }

  return result;
}

export function convertTagApiFieldsToFormFields(tag: Pick<SubmitRequestBody, 'name' | 'tagType' | 'meta'>): FormFieldTag {
  return {
    name: tag.name,
    type: [ tag.tagType ],
    url: tag.meta.tagUrl,
    iconUrl: tag.meta.tagIcon,
    bgColor: tag.meta.bgColor,
    textColor: tag.meta.textColor,
    tooltipDescription: tag.meta.tooltipDescription,
  };
}

export function groupSubmitResult(data: FormSubmitResult | undefined): FormSubmitResultGrouped | undefined {
  if (!data) {
    return;
  }

  const _items: Array<FormSubmitResultItemGrouped> = [];

  // group by error and address
  for (const item of data) {
    const existingItem = _items.find(({ error, addresses }) => error === item.error && addresses.length === 1 && addresses[0] === item.payload.address);
    if (existingItem) {
      existingItem.tags.push({ name: item.payload.name, tagType: item.payload.tagType, meta: item.payload.meta });
      continue;
    }

    _items.push({
      error: item.error,
      addresses: [ item.payload.address ],
      tags: [ { name: item.payload.name, tagType: item.payload.tagType, meta: item.payload.meta } ],
    });
  }

  const items: Array<FormSubmitResultItemGrouped> = [];

  // merge items with the same error and tags
  for (const item of _items) {
    const existingItem = items.find(({ error, tags }) => error === item.error && isEqual(tags, item.tags));
    if (existingItem) {
      existingItem.addresses.push(...item.addresses);
      continue;
    }

    items.push(item);
  }

  return {
    requesterName: data[0].payload.requesterName,
    requesterEmail: data[0].payload.requesterEmail,
    companyName: data[0].payload.companyName,
    companyWebsite: data[0].payload.companyWebsite,
    items: items.sort((a, b) => {
      if (a.error && !b.error) {
        return 1;
      }
      if (!a.error && b.error) {
        return -1;
      }
      return 0;
    }),
  };
}

export function getFormDefaultValues(query: Route['query'], userInfo: UserInfo | undefined) {
  return {
    addresses: getAddressesFromQuery(query),
    requesterName: getQueryParamString(query?.requesterName) || userInfo?.nickname || userInfo?.name || undefined,
    requesterEmail: getQueryParamString(query?.requesterEmail) || userInfo?.email || undefined,
    companyName: getQueryParamString(query?.companyName),
    companyWebsite: getQueryParamString(query?.companyWebsite),
    tags: [ { name: '', type: [ 'name' as const ] } ],
  };
}

function getAddressesFromQuery(query: Route['query']) {
  if (!query?.addresses) {
    return [ { hash: '' } ];
  }

  if (Array.isArray(query.addresses)) {
    return query.addresses.map((hash) => ({ hash }));
  }

  return [ { hash: query.addresses } ];
}
