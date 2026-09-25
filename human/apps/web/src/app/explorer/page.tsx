import { copyEntry } from "../../../copy/runtime";
import { explorerLink } from "../../explorer/client";
import { ExplorerFrame, ExplorerUnavailable } from "../../explorer/components";
import { ExplorerExternalLink, ExplorerLookupForm, ExplorerPanel } from "../../kit/explorer";
import { PlaneRouteAction } from "../../kit/plane-route-action";

export default function ExplorerPlanePage() {
  let anchors;
  try {
    anchors = explorerLink({ kind: "anchor" });
  } catch {
    return <ExplorerUnavailable />;
  }
  return (
    <ExplorerFrame
      title={copyEntry("explorer.title").message}
      description={copyEntry("explorer.summary").message}
    >
      <ExplorerPanel title={copyEntry("explorer.anchors.title").message}>
        <p className="text-sm text-foreground-secondary">
          {copyEntry("explorer.anchors.body").message}
        </p>
        <ExplorerExternalLink href={anchors}>
          {copyEntry("explorer.anchors.action").message}
        </ExplorerExternalLink>
      </ExplorerPanel>
      <div className="grid gap-4 lg:grid-cols-2">
        <ExplorerPanel title={copyEntry("explorer.lookup.receipt.title").message}>
          <ExplorerLookupForm
            action="/explorer/lookup"
            kind="receipt"
            label={copyEntry("explorer.lookup.receipt.label").message}
            placeholder={copyEntry("explorer.lookup.receipt.placeholder").message}
            submitLabel={copyEntry("explorer.lookup.action").message}
          />
        </ExplorerPanel>
        <ExplorerPanel title={copyEntry("explorer.lookup.account.title").message}>
          <ExplorerLookupForm
            action="/explorer/lookup"
            kind="account"
            label={copyEntry("explorer.lookup.account.label").message}
            placeholder={copyEntry("explorer.lookup.account.placeholder").message}
            submitLabel={copyEntry("explorer.lookup.action").message}
          />
        </ExplorerPanel>
        <ExplorerPanel title={copyEntry("explorer.lookup.program.title").message}>
          <ExplorerLookupForm
            action="/explorer/lookup"
            kind="program"
            label={copyEntry("explorer.lookup.program.label").message}
            placeholder={copyEntry("explorer.lookup.program.placeholder").message}
            submitLabel={copyEntry("explorer.lookup.action").message}
          />
        </ExplorerPanel>
      </div>
      <div>
        <PlaneRouteAction destination="/app">
          {copyEntry("action.open_app").message}
        </PlaneRouteAction>
      </div>
    </ExplorerFrame>
  );
}
