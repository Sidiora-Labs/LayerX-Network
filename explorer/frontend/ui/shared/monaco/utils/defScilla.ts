/* eslint-disable max-len */
import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';

export const configScilla: monaco.languages.LanguageConfiguration = {
  comments: {
    blockComment: [ '(*', '*)' ],
  },
  brackets: [
    [ '{', '}' ],
    [ '[', ']' ],
    [ '(', ')' ],
  ],
  autoClosingPairs: [
    { open: '"', close: '"', notIn: [ 'string', 'comment' ] },
    { open: '{', close: '}', notIn: [ 'string', 'comment' ] },
    { open: '[', close: ']', notIn: [ 'string', 'comment' ] },
    { open: '(', close: ')', notIn: [ 'string', 'comment' ] },
  ],
};

export const defScilla: monaco.languages.IMonarchLanguage = {
//   defaultToken: 'invalid',

  brackets: [
    { token: 'delimiter.curly', open: '{', close: '}' },
    { token: 'delimiter.parenthesis', open: '(', close: ')' },
    { token: 'delimiter.square', open: '[', close: ']' },
  ],

  keywords: [
    'let',
    'contains',
    'delete',
    'put',
    'remove',
    'library',
    'import',
    'contract',
    'event',
    'field',
    'send',
    'fun',
    'transition',
    'procedure',
    'match',
    'end',
    'with',
    'builtin',
    'Emp',
    'of',
    'scilla_version',
  ],

  builtins: [
    'eq',
    'add',
    'sub',
    'mul',
    'div',
    'rem',
    'lt',
    'blt',
    'in',
    'substr',
    'sha256hash',
    'keccak256hash',
    'ripemd160hash',
    'to_byStr',
    'to_nat',
    'pow',
    'to_uint256',
    'to_uint32',
    'to_uint64',
    'to_uint128',
    'to_int256',
    'to_int32',
    'to_int64',
    'to_int128',
    'schnorr_verify',
    'concat',
    'andb',
    'orb',
    'bool_to_string',
    'negb',
    'Nil',
  ],

  operators: [
    '=',
    '=>',
    '<=',
    '->',
    '<-',
  ],

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
        /[a-z_]\w*[!?=]?/i,
        {
          cases: {
            '@keywords': { token: 'keyword.$0' },
            '@builtins': 'predefined',
            '@default': 'identifier',
          },
        },
      ],

      // the rule above does not work for Nil and Emp for some reason
      // so we have to explicitly define them
      [ /\b(Nil)\b/, 'predefined' ],
      [ /\b(Emp)\b/, 'keyword' ],

      // types
      [ /\b(String|Uint32|Uint64|Uint128|Uint256|Int32|Int64|Int128|Int256|Map|True|False|ByStr|ByStr20|ByStr32|ByStr64|ByStr33|BNum|Option|None|Bool|Some|List|Cons|Pair|type|Zero|Succ|Message)\b/, 'type' ],

      // whitespace
      { include: '@whitespace' },

      // numbers
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

      // strings
      [ /"([^"\\]|\\.)*$/, 'string.invalid' ], // non-teminated string
      [ /"/, 'string', '@string' ],

      // characters
      [ /'[^\\']'/, 'string' ],
      [ /(')(@escapes)(')/, [ 'string', 'string.escape', 'string' ] ],
      [ /'/, 'string.invalid' ],
    ],
    whitespace: [
      [ /[ \t\r\n]+/, 'white' ],
      [ /\(\*.*$/, 'comment' ],
    ],
    comment: [
      [ /[^(*]+/, 'comment' ],
      [ /[(*]/, 'comment' ],
    ],
    string: [
      [ /[^\\"]+/, 'string' ],
      [ /@escapes/, 'string.escape' ],
      [ /\\./, 'string.escape.invalid' ],
      [ /"/, { token: 'string.quote', bracket: '@close', next: '@pop' } ],
    ],
  },
};
