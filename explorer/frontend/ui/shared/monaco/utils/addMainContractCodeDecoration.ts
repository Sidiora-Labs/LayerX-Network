import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';

import sortByEndLineNumberAsc from './sortByEndLineNumberAsc';

export default function addMainContractCodeDecoration(model: monaco.editor.ITextModel, contractName: string, editor: monaco.editor.IStandaloneCodeEditor) {
  const options: monaco.editor.IModelDecorationOptions = {
    isWholeLine: true,
  };

  const contractBlockMatches = model.findMatches(`^contract\\s`, false, true, false, null, true);

  if (contractBlockMatches.length < 2) {
    return;
  }

  const [ firstLineMatch ] = model.findMatches(`(^contract ${ contractName })(\\sis\\s.+(\\s?,.+)*)?((\\s?\\{)|\\n)`, false, true, false, null, true);

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
      className: '.main-contract-header',
      marginClassName: '.main-contract-header',
      glyphMarginClassName: '.main-contract-glyph',
      glyphMarginHoverMessage: [
        { value: 'Main contract' },
      ],
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

  const restDecoration: monaco.editor.IModelDeltaDecoration | undefined = lastLineMatch ? {
    range: {
      startLineNumber: firstLineMatch.range.startLineNumber + 1,
      endLineNumber: lastLineMatch.range.startLineNumber,
      startColumn: 1,
      endColumn: 10, // doesn't really matter since isWholeLine is true
    },
    options: {
      ...options,
      className: '.main-contract-body',
      marginClassName: '.main-contract-body',
    },
  } : undefined;

  editor.updateOptions({ glyphMargin: true });
  model.deltaDecorations([], [ firstLineDecoration, restDecoration ].filter(Boolean));
}
