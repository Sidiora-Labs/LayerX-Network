import * as v from 'valibot';

export const SolidityScanIssueSeverityDistributionSchema = v.object({
  critical: v.number(),
  gas: v.number(),
  high: v.number(),
  informational: v.number(),
  low: v.number(),
  medium: v.number(),
});

export const SolidityScanSchema = v.object({
  scan_report: v.object({
    contractname: v.string(),
    scan_status: v.string(),
    scan_summary: v.object({
      score_v2: v.string(),
      issue_severity_distribution: SolidityScanIssueSeverityDistributionSchema,
    }),
    scanner_reference_url: v.string(),
  }),
});

export type SolidityScanReport = v.InferOutput<typeof SolidityScanSchema>;
export type SolidityScanReportSeverityDistribution = v.InferOutput<typeof SolidityScanIssueSeverityDistributionSchema>;
