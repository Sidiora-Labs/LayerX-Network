export type StatsInterval = { id: StatsIntervalIds; title: string };
export type StatsIntervalIds = keyof typeof StatsIntervalId;
export enum StatsIntervalId {
  all,
  oneMonth,
  threeMonths,
  sixMonths,
  oneYear,
}
