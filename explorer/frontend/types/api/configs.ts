export interface BackendConfig {
  chain_type: string;
  openapi_spec_folder_name: string;
}

export interface BackendVersionConfig {
  backend_version: string;
}

export interface CsvExportConfig {
  limit: number;
}

export interface CeloConfig {
  l2_migration_block: number;
}

export interface ContractLanguagesConfig {
  languages: Array<string>;
}
