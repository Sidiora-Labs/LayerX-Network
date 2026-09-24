import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';

export default function addFileImportDecorations(model: monaco.editor.ITextModel) {
  const options: monaco.editor.IModelDecorationOptions = {
    inlineClassName: 'import-link',
    hoverMessage: {
      value: 'Cmd/Win + click to open file',
    },
  };

  const regularImportMatches = model.findMatches('^import (\'|")(.+)(\'|")', false, true, false, null, true);
  const regularImportDecorations: Array<monaco.editor.IModelDeltaDecoration> = regularImportMatches.map(({ range }) => ({
    range: {
      ...range,
      startColumn: range.startColumn + 8,
      endColumn: range.endColumn - 1,
    },
    options,
  }));

  const namedImportMatches = model.findMatches('(^import \\{.+\\} from )(\'|")(.+)(\'|")', false, true, false, null, true);
  const namedImportDecorations: Array<monaco.editor.IModelDeltaDecoration> = namedImportMatches.map(({ range, matches }) => ({
    range: {
      ...range,
      startColumn: range.startColumn + (Array.isArray(matches) ? matches?.[1]?.length + 1 : 0),
      endColumn: range.endColumn - 1,
    },
    options,
  }));

  // TODO: add support for "import * as" - https://docs.soliditylang.org/en/latest/grammar.html#a4.SolidityParser.importDirective
  // but we need a live example first to test it

  model.deltaDecorations([], regularImportDecorations.concat(namedImportDecorations));
}
