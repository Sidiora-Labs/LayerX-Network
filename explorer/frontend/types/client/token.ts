export interface Metadata {
  name?: string;
  description?: string;
  attributes?: Array<MetadataAttributes>;
}

export interface MetadataAttributes {
  value: string;
  trait_type: string;
  value_type?: 'URL';
}

export interface BridgedTokenChain {
  id: string;
  title: string;
  short_title: string;
  base_url: string;
}

export interface TokenBridge {
  type: string;
  title: string;
  short_title: string;
}

export interface AdditionalTokenType {
  id: string;
  name: string;
}
