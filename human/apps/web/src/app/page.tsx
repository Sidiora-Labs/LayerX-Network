import { cookies, headers } from "next/headers";

import { copyEntry } from "../../copy/runtime";
import { verifiedWebSession } from "../auth/server-session";
import { Onboarding } from "../journeys/onboarding/onboarding";
import { ExplorerNavigation } from "../kit/explorer";
import { PlaneRouteAction } from "../kit/plane-route-action";
import { selectServerShell } from "../shell/server";
import { MARKET_ROUTES } from "./_markets/navigation";

export default async function RootPage({
  searchParams,
}: Readonly<{
  searchParams: Promise<Readonly<{ return_to?: string | string[] | undefined }>>;
}>) {
  const [parameters, requestHeaders, requestCookies] = await Promise.all([
    searchParams,
    headers(),
    cookies(),
  ]);
  const returnTo = typeof parameters.return_to === "string" ? parameters.return_to : undefined;
  const session = await verifiedWebSession(requestHeaders.get("cookie") ?? "");
  return (
    <div className="flex flex-col gap-4">
      <Onboarding
        initialSelection={selectServerShell(requestHeaders, requestCookies)}
        initiallyAuthenticated={session !== undefined}
        {...(returnTo === undefined ? {} : { returnTo })}
      />
      <section className="flex flex-col gap-1">
        <p>{copyEntry("security.key_export.offer").message}</p>
        <p>{copyEntry("security.key_export.consequence").message}</p>
        <p>{copyEntry("security.key_export.custodial_opt_in").message}</p>
      </section>
      <PlaneRouteAction destination="/explorer">
        {copyEntry("action.open_explorer").message}
      </PlaneRouteAction>
      <ExplorerNavigation label="Markets" items={MARKET_ROUTES} />
    </div>
  );
}
