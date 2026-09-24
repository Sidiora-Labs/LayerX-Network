import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';

export const light: monaco.editor.IStandaloneThemeData = {
  base: 'vs' as const,
  inherit: true,
  rules: [
    { token: 'predefined', foreground: '#cd3131' },
  ],
  colors: {
    'editor.background': '#f5f5f6',
    'editorWidget.background': '#f5f5f6',

    'tab.activeBackground': '#f5f5f6',
    'tab.inactiveBackground': 'rgb(236, 236, 236)',
    'tab.activeForeground': '#101112', // black
    'tab.inactiveForeground': '#4a5568', // gray.600
    'tab.border': 'rgb(243, 243, 243)',

    'icon.foreground': '#616161',

    'input.foreground': '#616161',
    'input.background': '#fff',

    'list.inactiveSelectionBackground': '#e4e6f1',

    'breadcrumbs.foreground': 'rgb(97, 97, 97)',

    'badge.background': '#c4c4c4',

    'sideBar.background': '#eee',

    focusBorder: '#0090f1',

    // not able to use rgba for standard variables, so we use custom prefix here
    'custom.list.hoverBackground': 'rgba(16, 17, 18, 0.08)', // blackAlpha.200
    'custom.findMatchHighlightBackground': 'rgba(198, 246, 213, 1)',
    'custom.inputOption.activeBackground': 'rgba(0, 144, 241, 0.2)',
    'custom.inputOption.hoverBackground': 'rgba(184, 184, 184, 0.31)',

    // don't know the name of this variables in vscode
    'custom.fileLink.hoverForeground': '#4299E1', // blue.400
    'custom.riskWarning.primaryBackground': '#FEEBCB', // orange.100
    'custom.riskWarning.background': '#FFFAF0', // orange.50
    'custom.mainContract.header': 'rgba(233, 216, 253, 1)', // purple.100
    'custom.mainContract.body': 'rgba(250, 245, 255, 1)', // purple.50
  } as const,
};

export const dark: monaco.editor.IStandaloneThemeData = {
  base: 'vs-dark' as const,
  inherit: true,
  rules: [
    { token: 'predefined', foreground: '#f44747' },
  ],
  colors: {
    'editor.background': '#1a1b1b',
    'editorWidget.background': '#1a1b1b',

    'tab.activeBackground': '#1a1b1b', // black
    'tab.inactiveBackground': 'rgb(45, 45, 45)',
    'tab.activeForeground': '#fff', // white
    'tab.inactiveForeground': '#a0aec0', // gray.400
    'tab.border': 'rgb(37, 37, 38)',

    'icon.foreground': '#616161',

    'input.foreground': '#cccccc',
    'input.background': '#3c3c3c',

    'list.inactiveSelectionBackground': '#37373d',

    'badge.background': '#4d4d4d',

    'breadcrumbs.foreground': 'rgb(97, 97, 97)',

    'sideBar.background': '#222',

    focusBorder: '#007fd4',

    // not able to use rgba for standard variables, so we use custom prefix here
    'custom.list.hoverBackground': 'rgba(255, 255, 255, 0.08)', // whiteAlpha.200
    'custom.findMatchHighlightBackground': 'rgba(34, 84, 61, 1)',
    'custom.inputOption.activeBackground': 'rgba(0, 127, 212, 0.4)',
    'custom.inputOption.hoverBackground': 'rgba(90, 93, 94, 0.31)',

    // don't know the name of this variables in vscode
    'custom.fileLink.hoverForeground': '#4299E1', // blue.400
    'custom.riskWarning.primaryBackground': 'rgba(246, 173, 85, 0.3)', // orange.300
    'custom.riskWarning.background': 'rgba(246, 173, 85, 0.1)', // orange.300
    'custom.mainContract.header': 'rgba(183, 148, 244, 0.3)', // purple.300
    'custom.mainContract.body': 'rgba(214, 188, 250, 0.1)', // purple.200
  } as const,
};
