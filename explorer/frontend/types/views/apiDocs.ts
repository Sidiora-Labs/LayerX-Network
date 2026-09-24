export const API_DOCS_TABS = [
  'rest_api',
  'eth_rpc_api',
  'rpc_api',
  'graphql_api',
] as const;

export type ApiDocsTabId = typeof API_DOCS_TABS[ number ];
