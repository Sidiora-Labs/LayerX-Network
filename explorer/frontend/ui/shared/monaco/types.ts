import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';

export interface File {
  file_path: string;
  source_code: string;
}

export interface FileTreeFile extends File {
  name: string;
}

export interface FileTreeFolder {
  name: string;
  children: Array<FileTreeFile | FileTreeFolder>;
}

export type FileTree = Array<FileTreeFile | FileTreeFolder>;

export type Monaco = typeof monaco;

export interface SearchResult {
  file_path: string;
  matches: Array<
  Pick<monaco.editor.FindMatch['range'], 'startColumn' | 'endColumn' | 'startLineNumber' | 'endLineNumber'> &
  { lineContent: string }
  >;
}
