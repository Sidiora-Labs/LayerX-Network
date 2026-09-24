import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';

import type { SmartContractExternalLibrary } from 'types/api/contract';

import sortByEndLineNumberAsc from './sortByEndLineNumberAsc';

export default function addExternalLibraryWarningDecoration(model: monaco.editor.ITextModel, libraries: Array<SmartContractExternalLibrary>) {
  const options: monaco.editor.IModelDecorationOptions = {
    isWholeLine: true,
    hoverMessage: [
      { value: '**This is an external library linked to the verified contract**' },
      // eslint-disable-next-line max-len
      { value: 'The linked library source code only affects the bytecode part with external `DELEGATECALL` to the library and it is not possible to automatically ensure that provided library is really the one deployed at specified address. If you want to be sure, check the source code of the library at the given address. (See [issue](https://github.com/blockscout/blockscout-rs/issues/532) for more details)',
      },
    ],
  };

  const names = libraries.map(getLibraryName(model)).filter(Boolean).join('|');

  if (!names) {
    return;
  }

  const [ firstLineMatch ] = model.findMatches(`(^library ${ names })\\s?\\{`, false, true, false, null, true);

  if (!firstLineMatch) {
    return;
  }

  const firstLineDecoration: monaco.editor.IModelDeltaDecoration = {
    range: {
      startColumn: 1,
      endColumn: 10, // doesn't really matter since isWholeLine is true
      startLineNumber: firstLineMatch.range.startLineNumber,
      endLineNumber: firstLineMatch.range.startLineNumber,
    },
    options: {
      ...options,
      className: '.risk-warning-primary',
      marginClassName: '.risk-warning-primary',
    },
  };

  const lastLineRange: monaco.IRange = {
    startLineNumber: firstLineMatch.range.startLineNumber,
    startColumn: 1,
    endColumn: 10,
    endLineNumber: model.getLineCount(),
  };
  const [ lastLineMatch ] = model
    .findMatches(`^\\}`, lastLineRange, true, false, null, true)
    .sort(sortByEndLineNumberAsc);

  const restDecoration: monaco.editor.IModelDeltaDecoration = {
    range: {
      startLineNumber: firstLineMatch.range.startLineNumber + 1,
      endLineNumber: lastLineMatch.range.startLineNumber,
      startColumn: 1,
      endColumn: 10, // doesn't really matter since isWholeLine is true
    },
    options: {
      ...options,
      className: '.risk-warning',
      marginClassName: '.risk-warning',
    },
  };

  model.deltaDecorations([], [ firstLineDecoration, restDecoration ]);
}

const getLibraryName = (model: monaco.editor.ITextModel) => (library: SmartContractExternalLibrary) => {
  const containsFileName = library.name.includes(':');

  if (!containsFileName) {
    return library.name;
  }

  const [ fileName, libraryName ] = library.name.split(':');

  if (model.uri.path !== `/${ fileName }`) {
    return;
  }

  return libraryName;
};
