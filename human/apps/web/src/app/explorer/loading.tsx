import { copyEntry } from "../../../copy/runtime";
import { PerformanceLoadingCard } from "../../kit/performance";

export default function ExplorerLoading() {
  return (
    <PerformanceLoadingCard plane="explorer" label={copyEntry("state.loading").message} />
  );
}
