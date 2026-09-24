import { Box } from '@chakra-ui/react';
import { MultisenderWidget } from '@multisender.app/multisender-react-widget';
import React from 'react';

import { getFeaturePayload } from 'configs/app/features/types';

import config from 'configs/app';
import essentialDappsChainsConfig from 'configs/essential-dapps-chains';
import useIsMobile from 'lib/hooks/useIsMobile';
import AdBanner from 'ui/shared/ad/AdBanner';

const feature = getFeaturePayload(config.features.marketplace);
const dappConfig = feature?.essentialDapps?.multisend;

const Container = ({ children }: { children: React.ReactNode }) => (
  <Box
    w="full"
    maxW="670px"
    mx="auto"
    css={{
      '& > .multisenderTheme': {
        '--mw-color-brand': { _light: 'colors.blue.600', _dark: 'colors.blue.500' },
        '--mw-color-brand-stroke': { _light: 'colors.gray.200', _dark: 'colors.whiteAlpha.100' },
        '--mw-color-brand-text-secondary': 'colors.text.secondary',
        '--mw-color-brand-fill': 'colors.dialog.bg', // modal background
        fontFamily: 'body',
        '& [class*="_stepBody_"] > span': {
          fontSize: 'xs',
          fontWeight: 'normal',
        },
        '& [class*="_step_"]:not([data-progress="true"]), & [class*="_step_"][data-completed="true"]': {
          '& [class*="_stepBody_"] > span': {
            color: 'text.secondary',
          },
          '& [class*="_stepIcon_"]': {
            backgroundColor: { _light: 'gray.100', _dark: 'whiteAlpha.100' },
          },
          '& [class*="_stepCompletedIcon_"]': {
            color: 'text.secondary',
          },
        },
        '& [class*="_stepIcon_"]': {
          fontSize: 'xs',
          width: '24px',
          height: '24px',
          minWidth: '24px',
          minHeight: '24px',
        },
        '& [class*="_stepSeparator_"]': {
          height: '1px',
        },
        '& [class*="_stepperRoot_"] > :nth-child(1)': {
          padding: '0 12px',
        },
        '& [class*="_fieldsetRoot_"]:not([class*="_itemRoot_"])': {
          borderRadius: 'md',
          padding: '24px',
          '& button': {
            fontSize: 'xs',
            '& label': {
              fontWeight: 'medium !important',
            },
          },
          '& > *:first-child': {
            gap: '12px',
            '& > :nth-child(4) > button': {
              marginBottom: '12px',
              border: '2px solid',
              borderColor: 'button.outline.fg',
              borderRadius: 'base',
              color: 'button.outline.fg',
              height: '32px',
              padding: '0 12px',
              fontSize: 'sm',
              transitionProperty: 'all',
              transitionDuration: 'moderate',
              '&:hover': {
                background: 'none',
                color: 'hover',
                borderColor: 'hover',
              },
              '&:active': {
                transform: 'none',
              },
            },
          },
          '& > *:last-child': {
            '& p[data-size="sm"], & a[data-size="sm"]': {
              fontSize: 'xs',
            },
            '& a[data-size="sm"]': {
              color: 'link.primary',
              fontWeight: 'medium !important',
              '&:hover': {
                color: 'link.primary.hover',
              },
            },
          },
          '& p[data-size="xl"], & h5': {
            fontFamily: 'heading',
            fontSize: '18px',
            fontWeight: 'medium !important',
          },
          '& [class*="_tableTr_"]': {
            fontWeight: 'medium',
            '& td': {
              padding: '12px 0',
            },
            '& a': {
              color: 'link.primary',
              fontSize: 'sm',
              '&:hover': {
                color: 'link.primary.hover',
                textDecoration: 'none',
              },
            },
          },
          '& [class*="SummaryList-module__root__"]': {
            '& + div': {
              gap: '4px',
              '& > div:last-child': {
                '& button': {
                  background: 'none',
                  border: 'none',
                  color: 'button.outline.fg',
                  transitionProperty: 'all',
                  transitionDuration: 'moderate',
                  '&:hover': {
                    color: 'hover',
                  },
                  '&:active': {
                    transform: 'none',
                  },
                },
                '& p': {
                  fontSize: 'sm',
                  color: 'text.secondary',
                },
              },
            },
            '& + button': {
              marginTop: '12px',
            },
            '& span': {
              '--loader-color': 'var(--chakra-colors-text-primary) !important',
            },
          },
        },
        '& [class*="_itemRoot_"]': {
          border: 'none',
          borderRadius: 'base',
          backgroundColor: { _light: 'gray.50', _dark: 'whiteAlpha.100' },
          '& > div': {
            gap: '8px',
          },
        },
        '& [class*="_inputWrapper_"]': {
          '& [class*="_input_"]': {
            borderRadius: 'base',
            border: '2px solid',
            borderColor: 'input.border.filled',
            height: '32px',
            minHeight: '32px',
            fontSize: 'sm',
            fontWeight: 'medium',
            color: 'input.fg',
            '&::placeholder': {
              color: 'input.placeholder',
            },
          },
          '&[data-size="xl"]': {
            '& [class*="_input_"]': {
              height: '60px',
              minHeight: '60px',
              fontSize: 'md',
            },
            '& button': {
              height: '40px',
              fontSize: 'md',
              backgroundColor: 'button.solid.bg',
              borderRadius: 'base',
              transitionProperty: 'all',
              transitionDuration: 'moderate',
              color: 'white',
              '&:hover': {
                backgroundColor: 'hover',
              },
              '&:active': {
                transform: 'none',
              },
              '&:disabled': {
                opacity: '0.2',
              },
            },
          },
        },
        '& [class*="RecipientsEditor-module__head_"]': {
          fontSize: 'xs',
          marginTop: '12px',
          '& p': {
            fontWeight: 'medium',
          },
          '& button': {
            background: 'none',
            color: 'link.primary',
            fontWeight: 'medium',
            padding: 0,
            height: '18px',
            transitionProperty: 'all',
            transitionDuration: 'moderate',
            borderRadius: 0,
            '&:hover': {
              color: 'link.primary.hover',
            },
            '&:active': {
              transform: 'none',
            },
          },
        },
        '& [class*="CodeMirror"]': {
          fontSize: '15px',
          '& .cm-scroller': {
            borderRadius: 'base',
            borderWidth: '2px',
            borderColor: 'input.border.filled',
          },
          '& .cm-gutters': {
            borderTopLeftRadius: '0',
            borderBottomLeftRadius: '0',
            borderRightWidth: '2px',
            borderColor: 'input.border.filled',
            backgroundColor: { _light: 'gray.50', _dark: 'whiteAlpha.50' },
            backdropFilter: 'none',
            '&:before': {
              background: 'none',
            },
          },
          '& .cm-placeholder': {
            color: 'input.placeholder',
          },
        },
        '& [class*="_modalRoot_"]': {
          '& [class*="_modalOverlay_"]': {
            backgroundColor: 'blackAlpha.800',
          },
          '& [class*="_modalContent_"]': {
            borderRadius: 'xl',
          },
          '& [class*="_modalHeader_"]': {
            minHeight: '64px',
            height: '64px !important',
            padding: '24px !important',
            paddingBottom: '0 !important',
            marginBottom: '8px',
            '& [class*="_modalTitle_"]': {
              fontFamily: 'heading',
              fontWeight: 'medium !important',
              fontSize: '24px !important',
            },
          },
          '& [class*="_modalHeader_"] + div': {
            padding: '24px !important',
            paddingTop: '0 !important',
            '& > div': {
              padding: '0 !important',
            },
            '& h6, & p': {
              fontSize: 'md',
            },
          },
          '& [class*="_closeButton_"]': {
            background: 'none',
            color: 'close-button-fg',
            '&:hover': {
              color: 'hover',
            },
          },
        },
        '& div[role="alert"]': {
          borderRadius: 'base',
          padding: '8px 12px',
          margin: '0 !important',
          '& p, & div': {
            fontSize: 'md',
            color: 'text.primary !important',
          },
          '& [class*="_alertIcon_"]': {
            marginTop: '2px',
            marginRight: '8px',
            '& svg': {
              fill: 'text.primary',
              width: '20px',
              height: '20px',
            },
          },
          '&:not([data-variant="filled"]) [class*="_alertIcon_"]': {
            display: 'none',
          },
          '&[data-variant="filled"]': {
            backgroundColor: 'alert.bg.info !important',
          },
        },
        '& div[role="tooltip"]': {
          backgroundColor: 'tooltip.bg',
          color: 'tooltip.fg',
          borderRadius: 'sm',
          padding: '4px 8px !important',
          '& p': {
            fontSize: 'sm',
            fontWeight: 'medium',
          },
        },
        '& [class*="_button_"][data-size="lg"], [class*="_button_"][data-size="md"]': {
          borderRadius: 'base',
          color: 'white',
          height: '40px',
          backgroundColor: 'button.solid.bg',
          transitionProperty: 'background-color',
          transitionDuration: 'moderate',
          '&:disabled': {
            opacity: '0.2',
          },
          '&:hover': {
            backgroundColor: 'hover',
          },
          '&:active': {
            transform: 'none',
          },
          '& > span[aria-hidden="true"]': {
            width: '20px',
            height: '20px',
            '& > span::after': {
              width: '20px',
              height: '20px',
            },
          },
        },
        '& [class*="Summary-module__upgradeBtn__"]': {
          borderRadius: 'base',
          border: 'none',
          height: '32px',
          padding: '0 12px',
          fontSize: 'sm !important',
          fontWeight: 'semibold',
          transitionProperty: 'all',
          transitionDuration: 'moderate',
          '&[class*="_crown_"]': {
            color: 'white',
            backgroundColor: 'button.solid.bg',
            '&:hover': {
              backgroundColor: 'hover',
            },
          },
          '&[class*="_gift_"]': {
            color: 'button.outline.fg',
            border: '2px solid',
            borderColor: 'button.outline.fg',
            marginRight: '-4px',
            background: 'none',
            '&:hover': {
              color: 'hover',
              borderColor: 'hover',
            },
          },
          '& span[data-position="left"]': {
            display: 'none',
          },
        },
        '& [class*="Summary-module__editGasCost__"]': {
          border: 'none',
          background: 'none',
          '& svg': {
            width: '16px',
            height: '16px',
            color: 'link.primary',
            transitionProperty: 'all',
            transitionDuration: 'moderate',
          },
          '&:hover svg': {
            color: 'link.primary.hover',
          },
          '&:active': {
            transform: 'none',
          },
        },
        '& div[role="radiogroup"]': {
          '& > div': {
            gap: '8px',
          },
          '& [class*="LargeRadio-module__root_"]': {
            '& svg': {
              transform: 'none',
            },
            '& label': {
              '& p:not([data-size="sm"])': {
                fontWeight: 'semibold !important',
              },
              '& p[data-size="sm"]': {
                color: 'text.secondary',
              },
            },
            '& > div[class*="LargeRadio-module__body_"]': {
              border: 'none',
              borderRadius: 'base',
              backgroundColor: { _light: 'gray.50', _dark: 'whiteAlpha.100' },
            },
            '&:not([data-checked="true"]) input': {
              backgroundColor: 'transparent',
              border: '2px solid',
              borderColor: 'input.border.filled',
            },
            '& > div > div:not([class*="_labelWrapper_"])': {
              marginTop: '2px',
            },
            '& .tabler-icon-info-circle': {
              stroke: 'text.primary',
              width: '20px',
              height: '20px',
            },
          },
        },
        '& [class*="TokenSelect-module__grid_"]': {
          display: 'flex',
          justifyContent: 'flex-start',
          gap: '12px',
          flexWrap: 'wrap',
        },
        '& [class*="TokenSelect-module__chainsList_"]': {
          gap: 0,
          marginTop: '24px',
          '& > div': {
            display: 'flex',
            padding: '8px',
            margin: '0 -8px',
            borderRadius: 'base',
            cursor: 'pointer',
            '&:hover': {
              backgroundColor: 'selected.control.bg',
            },
            '& > [class*="_chainRow_"]': {
              flex: 1,
              '&:hover': {
                color: 'inherit',
              },
            },
            '& > svg': {
              width: '18px',
            },
          },
        },
        '& [class*="TokenSelect-module__stickySection_"]': {
          top: '16px',
          '& > div:last-child': {
            gap: '8px !important',
          },
        },
        '& [class*="TokenSelect-module__searchInput_"]': {
          marginTop: '8px',
        },
        '& [class*="TokenSelect-module__tokensList_"]': {
          gap: 0,
          '& svg > path:first-child': {
            stroke: 'text.primary',
          },
        },
        '& [class*="TokenSelect-module__tokenRow_"]': {
          padding: '8px',
          margin: '0 -8px',
          borderRadius: 'base',
          '&:hover': {
            backgroundColor: 'selected.control.bg',
          },
          '& [class*="_tokenIcon_"]': {
            width: '40px',
            height: '40px',
          },
        },
        '& > .multisenderMantineProvider': {
          '--mantine-color-text': 'text.primary',
          '--mantine-color-red-light': 'colors.alert.bg.error',
          '--mantine-color-blue-light': 'colors.alert.bg.info',
          '--mantine-color-blue-light-color': 'colors.text.secondary',
          '--mantine-color-blue-text': 'colors.hover',
          '--mantine-color-blue-outline': 'colors.hover',
          '--mantine-color-green-text': 'colors.green.500',
          '& > div:nth-child(1)': {
            padding: '0 !important',
            gap: 0,
            '& > :nth-child(1)': {
              margin: '0 !important',
            },
          },
        },
      },
    }}
  >{ children }</Box>
);

const widgetConfig = Object.fromEntries(dappConfig?.chains.map((chainId) => {
  const chainConfig = essentialDappsChainsConfig()?.chains.find((chain) => chain.id === chainId);
  const explorerUrl = chainConfig?.app_config?.app?.baseUrl;
  const apiUrl = chainConfig?.app_config?.apis?.general?.endpoint;
  return [
    chainId,
    {
      id: Number(chainId),
      blockExplorerUrl: {
        tx: `${ explorerUrl }/tx/`,
        address: `${ explorerUrl }/address/`,
      },
      rpcUrls: [ `${ apiUrl }/api/eth-rpc` ],
      blockScoutApiUrl: apiUrl,
    },
  ];
}) || []);

const Multisend = () => {
  const isMobile = useIsMobile();

  return (
    <>
      <Container>
        <MultisenderWidget
          config={ widgetConfig }
          logoType="minified"
          posthogKey={ dappConfig?.posthogKey }
          posthogHost={ dappConfig?.posthogHost }
          classNames={{
            theme: 'multisenderTheme',
            mantineProvider: 'multisenderMantineProvider',
          }}
        />
      </Container>
      { (feature?.essentialDappsAdEnabled && !isMobile) && (
        <AdBanner
          format="desktop"
          w="fit-content"
          borderRadius="md"
          overflow="hidden"
          mx="auto"
          mt={ 10 }
        />
      ) }
    </>
  );
};

export default Multisend;
