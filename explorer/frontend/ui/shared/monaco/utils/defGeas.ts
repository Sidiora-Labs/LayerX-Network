import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';

export const configGeas: monaco.languages.LanguageConfiguration = {
  comments: {
    lineComment: ';;',
  },
};

export const defGeas: monaco.languages.IMonarchLanguage = {

  builtins: [
    'abs',
    'selector',
    'keccak256',
    'address',
    'include',
    'assemble',
    'pragma',
    'define',
    'push',
    ...Array.from({ length: 32 }, (_, i) => `push${ i }`),
  ],

  keywords: [
    // geasEnvOpcode
    'address',
    'balance',
    'origin',
    'caller',
    'callvalue',
    'calldataload',
    'calldatasize',
    'calldatacopy',
    'codesize',
    'codecopy',
    'gasprice',
    'returndatasize',
    'returndatacopy',
    'blockhash',
    'coinbase',
    'timestamp',
    'number',
    'difficulty',
    'gaslimit',
    'chainid',
    'selfbalance',
    'basefee',

    // geasTrieOpcode
    'extcodesize',
    'extcodecopy',
    'extcodehash',
    'sload',
    'sstore',
    'selfdestruct',

    // geasCallOpcode
    'create',
    'call',
    'callcode',
    'delegatecall',
    'create2',
    'staticcall',
  ],

  operators: [
    // geasRegularOpcode
    'stop',
    'add',
    'mul',
    'sub',
    'div',
    'sdiv',
    'mod',
    'smod',
    'addmod',
    'mulmod',
    'exp',
    'signextend',
    'lt',
    'gt',
    'slt',
    'sgt',
    'eq',
    'iszero',
    'and',
    'or',
    'xor',
    'not',
    'byte',
    'shl',
    'shr',
    'sar',
    'keccak256',
    'pop',
    'mload',
    'mstore',
    'mstore8',
    'jump',
    'jumpi',
    'pc',
    'msize',
    'gas',
    'jumpdest',
    'revert',
    'invalid',
    'return',

    // geasRegularOpcode (dynamic)
    'dup',
    ...Array.from({ length: 16 }, (_, i) => `dup${ i }`),
    'swap',
    ...Array.from({ length: 16 }, (_, i) => `swap${ i }`),
    'log',
    ...Array.from({ length: 4 }, (_, i) => `log${ i }`),
  ],

  calls: /@\.\w+/,
  macros: /#?(include|assemble|push|abs|selector|keccak256|address|pragma|define)/,

  // we include these common regular expressions
  symbols: /[=><!~?:&|+\-*/^%]+/,
  escapes: /\\(?:[abfnrtv\\"']|x[0-9A-Fa-f]{1,4}|u[0-9A-Fa-f]{4}|U[0-9A-Fa-f]{8})/,
  integersuffix: /(ll|LL|[uUlL])?(ll|LL|[uUlL])?/,
  floatsuffix: /[fl]?/i,

  // numbers
  decpart: /\d(_?\d)*/,
  decimal: /0|@decpart/,

  tokenizer: {
    root: [
      // identifiers and keywords
      [
        /[a-z_]\w*/i,
        {
          cases: {
            '@keywords': { token: 'keyword.$0' },
            '@builtins': 'predefined',
            '@operators': 'type',
            '@default': 'identifier',
          },
        },
      ],

      [ /@calls/, { cases: { '@default': 'parameter' } } ],
      [ /@macros/, { cases: { '@default': 'predefined' } } ],

      // Strings
      [ /"/, { token: 'string.quote', bracket: '@open', next: '@string' } ],

      // whitespace
      { include: '@whitespace' },

      // numbers
      { include: '@numbers' },

    ],
    comment: [
      [ /[^;]+/, 'comment' ],
      [ /;;/, 'comment' ],
    ],
    whitespace: [
      [ /[ \t\r\n]+/, 'white' ],
      [ /;;.*$/, 'comment' ],
    ],
    string: [
      [ /[^\\"]+/, 'string' ],
      [ /@escapes/, 'string.escape' ],
      [ /\\./, 'string.escape.invalid' ],
      [ /"/, { token: 'string.quote', bracket: '@close', next: '@pop' } ],
    ],
    numbers: [
      [ /0x[0-9a-f](_?[0-9a-f])*/i, 'number.hex' ],
      [ /0[_o][0-7](_?[0-7])*/i, 'number.octal' ],
      [ /0b[01](_?[01])*/i, 'number.binary' ],
      [ /0[dD]@decpart/, 'number' ],
      [
        /@decimal((\.@decpart)?([eE][-+]?@decpart)?)/,
        {
          cases: {
            $1: 'number.float',
            '@default': 'number',
          },
        },
      ],
    ],
  },
};
