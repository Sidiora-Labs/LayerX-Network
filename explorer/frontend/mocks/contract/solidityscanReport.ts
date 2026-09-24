import type { SolidityScanReport } from 'lib/solidityScan/schema';

export const solidityscanReportAverage: SolidityScanReport = {
  scan_report: {
    contractname: 'foo',
    scan_status: 'scan_done',
    scan_summary: {
      issue_severity_distribution: {
        critical: 0,
        gas: 1,
        high: 0,
        informational: 0,
        low: 2,
        medium: 0,
      },
      score_v2: '72.22',
    },
    scanner_reference_url: 'https://solidityscan.com/quickscan/0xc1EF7811FF2ebFB74F80ed7423f2AdAA37454be2/blockscout/eth-goerli?ref=blockscout',
  },
};

export const solidityscanReportGreat: SolidityScanReport = {
  scan_report: {
    contractname: 'foo',
    scan_status: 'scan_done',
    scan_summary: {
      issue_severity_distribution: {
        critical: 0,
        gas: 0,
        high: 0,
        informational: 0,
        low: 0,
        medium: 0,
      },
      score_v2: '100',
    },
    scanner_reference_url: 'https://solidityscan.com/quickscan/0xc1EF7811FF2ebFB74F80ed7423f2AdAA37454be2/blockscout/eth-goerli?ref=blockscout',
  },
};

export const solidityscanReportLow: SolidityScanReport = {
  scan_report: {
    contractname: 'foo',
    scan_status: 'scan_done',
    scan_summary: {
      issue_severity_distribution: {
        critical: 2,
        gas: 1,
        high: 3,
        informational: 0,
        low: 2,
        medium: 10,
      },
      score_v2: '22.22',
    },
    scanner_reference_url: 'https://solidityscan.com/quickscan/0xc1EF7811FF2ebFB74F80ed7423f2AdAA37454be2/blockscout/eth-goerli?ref=blockscout',
  },
};
