import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';

export default function sortByEndLineNumberAsc(a: monaco.editor.FindMatch, b: monaco.editor.FindMatch) {
  if (a.range.endLineNumber < b.range.endLineNumber) {
    return -1;
  }

  if (a.range.endLineNumber > b.range.endLineNumber) {
    return 1;
  }

  return 0;
}
