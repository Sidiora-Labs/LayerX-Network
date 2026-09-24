import * as yup from 'yup';
import { CHAIN_INDICATOR_IDS, ChainIndicatorId, HeroBannerConfig, HeroBannerButtonState, HOME_STATS_WIDGET_IDS, HomeStatsWidgetId } from 'types/homepage';
import { replaceQuotes } from 'configs/app/utils';
import { getYupValidationErrorMessage, urlTest } from '../utils';
import { NavigationLayout, NavigationPromoBannerConfig, NavItemExternal } from 'types/client/navigation';
import { FeaturedNetwork, NETWORK_GROUPS, NetworkExplorer } from 'types/networks';
import { CustomLink, CustomLinksGroup } from 'types/footerLinks';
import { COLOR_THEME_IDS } from 'types/settings';
import { FontFamily } from 'types/ui';
import { ContractCodeIde, SMART_CONTRACT_EXTRA_VERIFICATION_METHODS, type SmartContractVerificationMethodExtra } from 'types/client/contract';
import type { AddressFormat, AddressViewId } from 'types/views/address';
import { ADDRESS_FORMATS, ADDRESS_VIEWS_IDS, IDENTICON_TYPES } from 'types/views/address';
import { BLOCK_FIELDS_IDS } from 'types/views/block';
import type { BlockFieldId } from 'types/views/block';
import type { TxAdditionalFieldsId, TxFieldsId } from 'types/views/tx';
import { TX_ADDITIONAL_FIELDS_IDS, TX_FIELDS_IDS } from 'types/views/tx';
import type { VerifiedContractsFilter } from 'types/api/contracts';
import * as regexp from 'toolkit/utils/regexp';
import { NftMarketplaceItem } from 'types/views/nft';

const heroBannerButtonStateSchema: yup.ObjectSchema<HeroBannerButtonState> = yup.object({
    background: yup.array().max(2).of(yup.string()),
    text_color: yup.array().max(2).of(yup.string()),
  });

const heroBannerSchema: yup.ObjectSchema<HeroBannerConfig> = yup.object()
    .transform(replaceQuotes)
    .json()
    .shape({
      background: yup.array().max(2).of(yup.string()),
      text_color: yup.array().max(2).of(yup.string()),
      border: yup.array().max(2).of(yup.string()),
      button: yup.object({
        _default: heroBannerButtonStateSchema,
        _hover: heroBannerButtonStateSchema,
        _selected: heroBannerButtonStateSchema,
      }),
      search: yup.object({
        border_width: yup.array().max(2).of(yup.string()),
      }),
    });

export const homepageSchema = yup.object({
    NEXT_PUBLIC_HOMEPAGE_CHARTS: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(yup.string<ChainIndicatorId>().oneOf(CHAIN_INDICATOR_IDS))
      .test(
        'stats-api-required',
        'NEXT_PUBLIC_STATS_API_HOST is required when daily_operational_txs is enabled in NEXT_PUBLIC_HOMEPAGE_CHARTS',
        function(value) {
          // daily_operational_txs is presented only in stats microservice
          if (value?.includes('daily_operational_txs')) {
            return Boolean(this.parent.NEXT_PUBLIC_STATS_API_HOST);
          }
          return true;
        }
      ),
    NEXT_PUBLIC_HOMEPAGE_STATS: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(yup.string<HomeStatsWidgetId>().oneOf(HOME_STATS_WIDGET_IDS))
      .test(
        'stats-api-required',
        'NEXT_PUBLIC_STATS_API_HOST is required when total_operational_txs is enabled in NEXT_PUBLIC_HOMEPAGE_STATS',
        function(value) {
          // total_operational_txs is presented only in stats microservice
          if (value?.includes('total_operational_txs')) {
            return Boolean(this.parent.NEXT_PUBLIC_STATS_API_HOST);
          }
          return true;
        }
      ),
    NEXT_PUBLIC_HOMEPAGE_HERO_BANNER_CONFIG: yup
      .mixed()
      .test(
        'shape',
        (ctx) => {
          try {
            heroBannerSchema.validateSync(ctx.originalValue);
            throw new Error('Unknown validation error');
          } catch (error: unknown) {
            const message = getYupValidationErrorMessage(error);
            return 'Invalid schema were provided for NEXT_PUBLIC_HOMEPAGE_HERO_BANNER_CONFIG' + (message ? `: ${ message }` : '');
          }
        },
        (data) => {
          const isUndefined = data === undefined;
          return isUndefined || heroBannerSchema.isValidSync(data);
        }),
});

const featuredNetworkSchema: yup.ObjectSchema<FeaturedNetwork> = yup
  .object()
  .shape({
    title: yup.string().required(),
    url: yup.string().test(urlTest).required(),
    group: yup.string().oneOf(NETWORK_GROUPS).required(),
    icon: yup.string().test(urlTest),
    isActive: yup.boolean(),
    invertIconInDarkMode: yup.boolean(),
  });

const navItemExternalSchema: yup.ObjectSchema<NavItemExternal> = yup
  .object({
    text: yup.string().required(),
    url: yup.string().test(urlTest).required(),
  });

export const navigationSchema = yup.object({
    NEXT_PUBLIC_FEATURED_NETWORKS: yup
      .array()
      .json()
      .of(featuredNetworkSchema),
    NEXT_PUBLIC_FEATURED_NETWORKS_ALL_LINK: yup
      .string()
      .when('NEXT_PUBLIC_FEATURED_NETWORKS', {
        is: (value: Array<unknown> | undefined) => value && value.length > 0,
        then: (schema) => schema.test(urlTest),
        otherwise: (schema) => schema.max(-1,  'NEXT_PUBLIC_FEATURED_NETWORKS_ALL_LINK can only be set when NEXT_PUBLIC_FEATURED_NETWORKS is configured'),
      }),
    NEXT_PUBLIC_FEATURED_NETWORKS_MODE: yup
      .string()
      .when('NEXT_PUBLIC_FEATURED_NETWORKS', {
        is: (value: Array<unknown> | undefined) => value && value.length > 0,
        then: (schema) => schema.oneOf([ 'tabs', 'list' ]),
        otherwise: (schema) => schema.max(-1,  'NEXT_PUBLIC_FEATURED_NETWORKS_MODE can only be set when NEXT_PUBLIC_FEATURED_NETWORKS is configured'),
      }),
    NEXT_PUBLIC_OTHER_LINKS: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(navItemExternalSchema),
    NEXT_PUBLIC_NAVIGATION_HIGHLIGHTED_ROUTES: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(yup.string()),
    NEXT_PUBLIC_NAVIGATION_LAYOUT: yup.string<NavigationLayout>().oneOf([ 'horizontal', 'vertical' ]),
    NEXT_PUBLIC_NAVIGATION_PROMO_BANNER_CONFIG: yup
      .mixed()
      .test('shape', 'Invalid schema were provided for NEXT_PUBLIC_NAVIGATION_PROMO_BANNER_CONFIG, it should be either object with img_url, text, bg_color, text_color, link_url or object with img_url and link_url', (data) => {
        const isUndefined = data === undefined;
        const jsonSchema = yup.object<NavigationPromoBannerConfig>().transform(replaceQuotes).json();

        const valueSchema1 = jsonSchema.shape({
          img_url: yup.string().required(),
          text: yup.string().required(),
          bg_color: yup.object().shape({
            light: yup.string().required(),
            dark: yup.string().required(),
          }).required(),
          text_color: yup.object().shape({
            light: yup.string().required(),
            dark: yup.string().required(),
          }).required(),
          link_url: yup.string().required(),
        });

        const valueSchema2 = jsonSchema.shape({
          img_url: yup.object().shape({
            small: yup.string().required(),
            large: yup.string().required(),
          }).required(),
          link_url: yup.string().required(),
        });

        return isUndefined || valueSchema1.isValidSync(data) || valueSchema2.isValidSync(data);
      }),
    NEXT_PUBLIC_NETWORK_LOGO: yup.string().test(urlTest),
    NEXT_PUBLIC_NETWORK_LOGO_DARK: yup.string().test(urlTest),
    NEXT_PUBLIC_NETWORK_ICON: yup.string().test(urlTest),
    NEXT_PUBLIC_NETWORK_ICON_DARK: yup.string().test(urlTest),
});

const footerLinkSchema: yup.ObjectSchema<CustomLink> = yup
  .object({
    text: yup.string().required(),
    url: yup.string().test(urlTest).required(),
    iconUrl: yup.array().of(yup.string().required().test(urlTest)),
  });

const footerLinkGroupSchema: yup.ObjectSchema<CustomLinksGroup> = yup
  .object({
    title: yup.string().required(),
    links: yup
      .array()
      .of(footerLinkSchema)
      .required(),
  });

export const footerSchema = yup.object({
    NEXT_PUBLIC_FOOTER_LINKS: yup
      .array()
      .json()
      .of(footerLinkGroupSchema),
});

const fontFamilySchema: yup.ObjectSchema<FontFamily> = yup
  .object()
  .transform(replaceQuotes)
  .json()
  .shape({
    name: yup.string().required(),
    url: yup.string().test(urlTest).required(),
  });

export const miscSchema = yup.object({
    NEXT_PUBLIC_HIDE_INDEXING_ALERT_BLOCKS: yup.boolean(),
    NEXT_PUBLIC_HIDE_INDEXING_ALERT_INT_TXS: yup.boolean(),
    NEXT_PUBLIC_HIDE_NATIVE_COIN_PRICE: yup.boolean(),
    NEXT_PUBLIC_MAINTENANCE_ALERT_MESSAGE: yup.string(),
    NEXT_PUBLIC_COLOR_THEME_DEFAULT: yup.string().oneOf(COLOR_THEME_IDS),
    NEXT_PUBLIC_COLOR_THEME_OVERRIDES: yup.object().transform(replaceQuotes).json(),
    NEXT_PUBLIC_FONT_FAMILY_HEADING: yup
      .mixed()
      .test('shape', 'Invalid schema were provided for NEXT_PUBLIC_FONT_FAMILY_HEADING', (data) => {
        const isUndefined = data === undefined;
        return isUndefined || fontFamilySchema.isValidSync(data);
      }),
    NEXT_PUBLIC_FONT_FAMILY_BODY: yup
      .mixed()
      .test('shape', 'Invalid schema were provided for NEXT_PUBLIC_FONT_FAMILY_BODY', (data) => {
        const isUndefined = data === undefined;
        return isUndefined || fontFamilySchema.isValidSync(data);
      }),
    NEXT_PUBLIC_MAX_CONTENT_WIDTH_ENABLED: yup.boolean(),
});

const networkExplorerSchema: yup.ObjectSchema<NetworkExplorer> = yup
  .object({
    title: yup.string().required(),
    logo: yup.string().test(urlTest),
    baseUrl: yup.string().test(urlTest).required(),
    paths: yup
      .object()
      .shape({
        tx: yup.string(),
        address: yup.string(),
        token: yup.string(),
        block: yup.string(),
        blob: yup.string(),
      }),
  });

const contractCodeIdeSchema: yup.ObjectSchema<ContractCodeIde> = yup
  .object({
    title: yup.string().required(),
    url: yup.string().test(urlTest).required(),
    icon_url: yup.string().test(urlTest).required(),
  });

const nftMarketplaceSchema: yup.ObjectSchema<NftMarketplaceItem> = yup
  .object({
    name: yup.string().required(),
    collection_url: yup.string().test(urlTest),
    instance_url: yup.string().test(urlTest),
    logo_url: yup.string().test(urlTest).required(),
  });


export const viewsSchema = yup.object({
  NEXT_PUBLIC_VIEWS_BLOCK_HIDDEN_FIELDS: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(yup.string<BlockFieldId>().oneOf(BLOCK_FIELDS_IDS)),
    NEXT_PUBLIC_VIEWS_BLOCK_PENDING_UPDATE_ALERT_ENABLED: yup.boolean(),
    NEXT_PUBLIC_VIEWS_ADDRESS_IDENTICON_TYPE: yup.string().oneOf(IDENTICON_TYPES),
    NEXT_PUBLIC_VIEWS_ADDRESS_FORMAT: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(yup.string<AddressFormat>().oneOf(ADDRESS_FORMATS)),
    NEXT_PUBLIC_VIEWS_ADDRESS_BECH_32_PREFIX: yup
      .string()
      .when('NEXT_PUBLIC_VIEWS_ADDRESS_FORMAT', {
        is: (value: Array<AddressFormat> | undefined) => value && value.includes('bech32'),
        then: (schema) => schema.required().min(1).max(83),
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_VIEWS_ADDRESS_BECH_32_PREFIX is required if NEXT_PUBLIC_VIEWS_ADDRESS_FORMAT contains "bech32"'),
      }),
    NEXT_PUBLIC_VIEWS_ADDRESS_NATIVE_TOKEN_ADDRESS: yup
      .string()
      .min(42)
      .max(42)
      .matches(regexp.HEX_REGEXP_WITH_0X),

    NEXT_PUBLIC_VIEWS_ADDRESS_HIDDEN_VIEWS: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(yup.string<AddressViewId>().oneOf(ADDRESS_VIEWS_IDS)),
    NEXT_PUBLIC_VIEWS_CONTRACT_SOLIDITYSCAN_ENABLED: yup.boolean(),
    NEXT_PUBLIC_VIEWS_CONTRACT_DECODED_BYTECODE_ENABLED: yup.boolean(),
    NEXT_PUBLIC_VIEWS_CONTRACT_EXTRA_VERIFICATION_METHODS: yup
      .mixed()
      .test(
        'shape',
        'Invalid schema were provided for NEXT_PUBLIC_VIEWS_CONTRACT_EXTRA_VERIFICATION_METHODS, it should be either array of method ids or "none" string literal',
        (data) => {
          const isNoneSchema = yup.string().oneOf([ 'none' ]);
          const isArrayOfMethodsSchema = yup
            .array()
            .transform(replaceQuotes)
            .json()
            .of(yup.string<SmartContractVerificationMethodExtra>().oneOf(SMART_CONTRACT_EXTRA_VERIFICATION_METHODS));

          return isNoneSchema.isValidSync(data) || isArrayOfMethodsSchema.isValidSync(data);
        }),

    NEXT_PUBLIC_VIEWS_TX_HIDDEN_FIELDS: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(yup.string<TxFieldsId>().oneOf(TX_FIELDS_IDS)),
    NEXT_PUBLIC_VIEWS_TX_ADDITIONAL_FIELDS: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(yup.string<TxAdditionalFieldsId>().oneOf(TX_ADDITIONAL_FIELDS_IDS)),
    NEXT_PUBLIC_VIEWS_TX_GROUPED_FEES: yup.boolean(),
    NEXT_PUBLIC_VIEWS_NFT_MARKETPLACES: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(nftMarketplaceSchema),
    NEXT_PUBLIC_VIEWS_TOKEN_SCAM_TOGGLE_ENABLED: yup.boolean(),
    NEXT_PUBLIC_HELIA_VERIFIED_FETCH_ENABLED: yup.boolean(),

    NEXT_PUBLIC_NETWORK_EXPLORERS: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(networkExplorerSchema),
    NEXT_PUBLIC_CONTRACT_CODE_IDES: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(contractCodeIdeSchema),
    NEXT_PUBLIC_HAS_CONTRACT_AUDIT_REPORTS: yup.boolean(),
});
