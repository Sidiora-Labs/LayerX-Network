import { padStart } from 'es-toolkit/compat';

export default function splitSecondsInPeriods(value: number) {
  const seconds = value % 60;
  const minutes = (value - seconds) / 60 % 60;
  const hours = (value - seconds - minutes * 60) / (60 * 60) % 24;
  const days = (value - seconds - minutes * 60 - hours * 60 * 60) / (60 * 60 * 24);

  return {
    seconds: padStart(String(seconds), 2, '0'),
    minutes: padStart(String(minutes), 2, '0'),
    hours: padStart(String(hours), 2, '0'),
    days: padStart(String(days), 2, '0'),
  };
}
