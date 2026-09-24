import { test, expect } from 'vitest';

import composeFileTree from './composeFileTree';

const files = [
  {
    file_path: 'index.sol',
    source_code: 'zero',
  },
  {
    file_path: 'contracts/Zeta.eth.sol',
    source_code: 'one',
  },
  {
    file_path: '/_openzeppelin/contracts/utils/Context.sol',
    source_code: 'two',
  },
  {
    file_path: '/_openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol',
    source_code: 'three',
  },
  {
    file_path: '/_openzeppelin/contracts/token/ERC20/IERC20.sol',
    source_code: 'four',
  },
  {
    file_path: '/_openzeppelin/contracts/token/ERC20/ERC20.sol',
    source_code: 'five',
  },
];

test('builds correct file tree', () => {
  const result = composeFileTree(files);
  expect(result).toMatchInlineSnapshot(`
[
  {
    "children": [
      {
        "children": [
          {
            "children": [
              {
                "children": [
                  {
                    "children": [
                      {
                        "file_path": "/_openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol",
                        "name": "IERC20Metadata.sol",
                        "source_code": "three",
                      },
                    ],
                    "name": "extensions",
                  },
                  {
                    "file_path": "/_openzeppelin/contracts/token/ERC20/ERC20.sol",
                    "name": "ERC20.sol",
                    "source_code": "five",
                  },
                  {
                    "file_path": "/_openzeppelin/contracts/token/ERC20/IERC20.sol",
                    "name": "IERC20.sol",
                    "source_code": "four",
                  },
                ],
                "name": "ERC20",
              },
            ],
            "name": "token",
          },
          {
            "children": [
              {
                "file_path": "/_openzeppelin/contracts/utils/Context.sol",
                "name": "Context.sol",
                "source_code": "two",
              },
            ],
            "name": "utils",
          },
        ],
        "name": "contracts",
      },
    ],
    "name": "_openzeppelin",
  },
  {
    "children": [
      {
        "file_path": "contracts/Zeta.eth.sol",
        "name": "Zeta.eth.sol",
        "source_code": "one",
      },
    ],
    "name": "contracts",
  },
  {
    "file_path": "index.sol",
    "name": "index.sol",
    "source_code": "zero",
  },
]
`);
});
