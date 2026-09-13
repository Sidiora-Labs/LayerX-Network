import { copyEntry } from "../../../copy/runtime";
import { PerformanceLoadingCard } from "../../kit/performance";

export default function AppLoading() {
  return (
    <PerformanceLoadingCard plane="app" label={copyEntry("state.loading").message} />
  );
}
