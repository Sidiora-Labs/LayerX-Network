import { describe, it, expect } from 'vitest';

import * as mocks from './mocks';
import { convertFormDataToRequestsBody, convertTagApiFieldsToFormFields, groupSubmitResult } from './utils';

describe('function convertFormDataToRequestsBody()', () => {
  it('should convert form data to requests body', () => {
    const formData = {
      ...mocks.baseFields,
      addresses: [ { hash: mocks.address1 }, { hash: mocks.address2 } ],
      tags: [ convertTagApiFieldsToFormFields(mocks.tag1), convertTagApiFieldsToFormFields(mocks.tag2) ],
    };
    const result = convertFormDataToRequestsBody(formData);
    expect(result).toMatchObject([
      { address: mocks.address1, name: mocks.tag1.name, tagType: mocks.tag1.tagType },
      { address: mocks.address1, name: mocks.tag2.name, tagType: mocks.tag2.tagType },
      { address: mocks.address2, name: mocks.tag1.name, tagType: mocks.tag1.tagType },
      { address: mocks.address2, name: mocks.tag2.name, tagType: mocks.tag2.tagType },
    ]);
  });
});

describe('function groupSubmitResult()', () => {
  it('group success result', () => {
    const result = groupSubmitResult(mocks.allSuccessResponses);
    expect(result).toMatchObject({
      requesterName: mocks.baseFields.requesterName,
      requesterEmail: mocks.baseFields.requesterEmail,
      companyName: mocks.baseFields.companyName,
      companyWebsite: mocks.baseFields.companyWebsite,
      items: [
        {
          error: null,
          addresses: [ mocks.address1, mocks.address2, mocks.address3, mocks.address4, mocks.address5 ],
          tags: [ mocks.tag1, mocks.tag2, mocks.tag3 ],
        },
      ],
    });
  });

  it('group result with error', () => {
    const result = groupSubmitResult(mocks.mixedResponses);
    expect(result).toMatchObject({
      requesterName: mocks.baseFields.requesterName,
      requesterEmail: mocks.baseFields.requesterEmail,
      companyName: mocks.baseFields.companyName,
      companyWebsite: mocks.baseFields.companyWebsite,
      items: [
        {
          error: null,
          addresses: [ mocks.address1 ],
          tags: [ mocks.tag1 ],
        },
        {
          error: null,
          addresses: [ mocks.address3 ],
          tags: [ mocks.tag3 ],
        },
        {
          error: 'Some error',
          addresses: [ mocks.address1, mocks.address2 ],
          tags: [ mocks.tag2, mocks.tag3 ],
        },
        {
          error: 'Some error',
          addresses: [ mocks.address3 ],
          tags: [ mocks.tag1 ],
        },
        {
          error: 'Another nasty error',
          addresses: [ mocks.address3 ],
          tags: [ mocks.tag2 ],
        },
      ],
    });
  });
});
