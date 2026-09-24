export type MultichainProviderConfig = {
  name: string;
  dapp_id?: string;
  url_template: string;
  logo: string;
  promo?: boolean;
};

export type MultichainProviderConfigParsed = {
  name: string;
  logoUrl: string;
  urlTemplate: string;
  dappId?: string;
  promo?: boolean;
};
