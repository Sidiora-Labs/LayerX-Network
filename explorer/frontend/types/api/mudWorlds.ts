import type { AddressParam } from './addressParams';

export type MudWorldsResponse = {
  items: Array<MudWorldItem>;
  next_page_params: {
    items_count: number;
    world: string;
  };
};

export type MudWorldItem = {
  address: AddressParam;
  coin_balance: string;
  transactions_count: number | null;
};

export type MudWorldSchema = {
  key_names: Array<string>;
  key_types: Array<string>;
  value_names: Array<string>;
  value_types: Array<string>;
};

export type MudWorldTable = {
  table_full_name: string;
  table_id: string;
  table_name: string;
  table_namespace: string;
  table_type: string;
};
