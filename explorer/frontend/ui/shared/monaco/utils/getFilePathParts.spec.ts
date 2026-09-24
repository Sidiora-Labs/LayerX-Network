import { it, expect } from 'vitest';

import getFilePathParts from './getFilePathParts';

it('computes correct chunks if all file name are unique', () => {
  const result = getFilePathParts(
    '/src/utils/Ownable.sol',
    [
      '/src/utils/EIP712.sol'.split('/'),
      '/src/token/BaseERC20.sol'.split('/'),
    ],
  );

  expect(result).toEqual([ 'Ownable.sol', undefined ]);
});

it('computes correct chunks if files with the same name is not in folders with the same name', () => {
  const result = getFilePathParts(
    '/src/utils/Ownable.sol',
    [
      '/src/utils/EIP712.sol'.split('/'),
      '/lib/_openzeppelin/contracts/contracts/access/Ownable.sol'.split('/'),
    ],
  );

  expect(result).toEqual([ 'Ownable.sol', '/utils' ]);
});

it('computes correct chunks if files with the same name is in folders with the same name', () => {
  const result = getFilePathParts(
    '/src/utils/Ownable.sol',
    [
      '/src/utils/EIP712.sol'.split('/'),
      '/lib/_openzeppelin/contracts/src/utils/Ownable.sol'.split('/'),
    ],
  );

  expect(result).toEqual([ 'Ownable.sol', './src/utils' ]);
});

it('computes correct chunks if file is in root directory', () => {
  const result = getFilePathParts(
    '/Ownable.sol',
    [
      '/src/utils/EIP712.sol'.split('/'),
      '/lib/_openzeppelin/contracts/src/utils/Ownable.sol'.split('/'),
    ],
  );

  expect(result).toEqual([ 'Ownable.sol', './' ]);
});
