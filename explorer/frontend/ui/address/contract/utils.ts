export const CONTRACT_MAIN_TAB_IDS = [
  'contract_code',
  'read_contract',
  'write_contract',
  'read_write_contract',
  'read_proxy',
  'write_proxy',
  'read_write_proxy',
  'read_custom_methods',
  'write_custom_methods',
  'read_write_custom_methods',
  'mud_system',
] as const;

export const CONTRACT_DETAILS_TAB_IDS = [
  'contract_source_code',
  'contract_compiler',
  'contract_abi',
  'contract_bytecode',
] as const;

export const CONTRACT_TAB_IDS = (CONTRACT_MAIN_TAB_IDS as unknown as Array<string>).concat(CONTRACT_DETAILS_TAB_IDS as unknown as Array<string>);
