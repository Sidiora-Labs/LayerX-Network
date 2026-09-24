export default function delay(time: number) {
  return new Promise((resolve) => window.setTimeout(resolve, time));
}
