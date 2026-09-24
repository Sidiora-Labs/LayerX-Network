import type CspDev from 'csp-dev';

import { KEY_WORDS } from '../utils';

export function monaco(): CspDev.DirectiveDescriptor {
  return {
    'script-src': [
      KEY_WORDS.BLOB,
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/loader.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/editor/editor.main.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/editor/editor.main.nls.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/basic-languages/solidity/solidity.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/basic-languages/elixir/elixir.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/basic-languages/javascript/javascript.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/basic-languages/typescript/typescript.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/basic-languages/rust/rust.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/language/json/jsonMode.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/language/json/jsonWorker.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/language/typescript/tsMode.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/language/typescript/tsWorker.js',
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/base/worker/workerMain.js',
    ],
    'style-src': [
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/editor/editor.main.css',
    ],
    'font-src': [
      'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs/base/browser/ui/codicons/codicon/codicon.ttf',
    ],
  };
}
