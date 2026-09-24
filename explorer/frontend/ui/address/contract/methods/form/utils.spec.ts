import { describe, it, expect } from 'vitest';

import { transformFormDataToMethodArgs } from './utils';

describe('transformFormDataToMethodArgs', () => {
  it('should transform form data to method args array', () => {
    const formData = {
      '1': '1',
      '2': '2',
      '0:1': '0:1',
      '0:0:0': '0:0:0',
      '0:0:1:0': '0:0:1:0',
      '0:0:1:3': '0:0:1:3',
      '0:0:2:1:0': '0:0:2:1:0',
      '0:0:2:1:1': '0:0:2:1:1',
      '0:0:2:2:0': '0:0:2:2:0',
      '0:0:2:2:2': '0:0:2:2:2',
      '0:0:2:5:3': '0:0:2:5:3',
      '0:0:2:5:8': '0:0:2:5:8',
    };
    const result = transformFormDataToMethodArgs(formData);
    expect(result).toEqual([
      [
        [
          '0:0:0',
          [
            '0:0:1:0',
            '0:0:1:3',
          ],
          [
            [
              '0:0:2:1:0',
              '0:0:2:1:1',
            ],
            [
              '0:0:2:2:0',
              '0:0:2:2:2',
            ],
            [
              '0:0:2:5:3',
              '0:0:2:5:8',
            ],
          ],
        ],
        '0:1',
      ],
      '1',
      '2',
    ]);
  });

  it('should leave the arg if it is an empty array', () => {
    const formData = {
      // simple array
      '0:0': undefined,

      // nested array
      '1:0:0': undefined,
      '1:1:0': '1',
      '1:1:1': '2',

      // array in a tuple
      '2:0': 'duck',
      '2:1:0': undefined,
    };
    const result = transformFormDataToMethodArgs(formData);
    expect(result).toEqual([
      [],
      [
        [],
        [ '1', '2' ],
      ],
      [
        'duck',
        [],
      ],
    ]);
  });

  it('should transform all nested empty arrays to empty arrays', () => {
    const formData = {
      '0': '0x1D415D28380ff51A507F7B176ca5F27833F7FffD',
      '1': '0x1D415D28380ff51A507F7B176ca5F27833F7FffD',
      '2': '3160',
      '3': true,
      // tuple array without elements
      '4:0:0:0': undefined,
      '4:0:1:0': undefined,
      '4:0:1:1': undefined,
      '4:0:1:2': undefined,
      '4:0:1:3': undefined,
    };
    const result = transformFormDataToMethodArgs(formData);
    expect(result).toEqual([
      '0x1D415D28380ff51A507F7B176ca5F27833F7FffD',
      '0x1D415D28380ff51A507F7B176ca5F27833F7FffD',
      '3160',
      true,
      [],
    ]);
  });

  it('should cast empty strings', () => {
    const formData = {
      '0': '""',
      '1': '0x1D415D28380ff51A507F7B176ca5F27833F7FffD',
      '2': '3160',
      '3': true,
      // nested elements
      '4:0:0:0': undefined,
      '4:0:1:0': '', // <<< not real case, the form will not allow to submit this value
      '4:0:1:1': '""',
      '4:0:1:2': '0',
      '4:0:1:3': false,
    };
    const result = transformFormDataToMethodArgs(formData);
    expect(result).toEqual([
      '',
      '0x1D415D28380ff51A507F7B176ca5F27833F7FffD',
      '3160',
      true,
      [
        [
          [],
          [ '', '', '0', false ],
        ],
      ],
    ]);
  });
});
