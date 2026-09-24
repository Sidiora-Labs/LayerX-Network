import type { MudWorldItem, MudWorldSchema, MudWorldTable } from 'types/api/mudWorlds';

import { ADDRESS_PARAMS } from './addressParams';

export const MUD_TABLE: MudWorldTable = {
  table_full_name: 'ot.Match',
  table_id: '0x6f7400000000000000000000000000004d617463680000000000000000000000',
  table_name: 'Match',
  table_namespace: '',
  table_type: 'offchain',
};

export const MUD_SCHEMA: MudWorldSchema = {
  key_names: [ 'matchEntityKey', 'entity' ],
  key_types: [ 'bytes32', 'bytes32' ],
  value_names: [ 'matchEntity' ],
  value_types: [ 'bytes32' ],
};

export const MUD_WORLD: MudWorldItem = {
  address: ADDRESS_PARAMS,
  coin_balance: '7072643779453701031672',
  transactions_count: 442,
};
