import type React from 'react';

export interface TabItemRegular {
  // NOTE, in case of array of ids, when switching tabs, the first id will be used
  // switching between other ids should be handled in the underlying component
  id: string | Array<string>;
  title: string | (() => React.ReactNode);
  count?: number | null;
  component: React.ReactNode;
  subTabs?: Array<string>;
}

export interface TabItemMenu {
  id: 'menu';
  title: string;
  count?: never;
  component: null;
}

export type TabItem = TabItemRegular | TabItemMenu;

export type SubTabItem = Omit<TabItem, 'subTabs'>;
