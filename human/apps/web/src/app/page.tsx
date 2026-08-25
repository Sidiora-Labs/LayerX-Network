import { cookies, headers } from "next/headers";

import { copyEntry } from "../../copy/catalog";
import { verifiedWebSession } from "../auth/server-session";
import { Onboarding } from "../journeys/onboarding/onboarding";
import { PlaneRouteAction } from "../kit";
import { selectServerShell } from "../shell/server";

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
      <PlaneRouteAction destination="/explorer">
        {copyEntry("action.open_explorer").message}
      </PlaneRouteAction>
    </div>
  );
}
