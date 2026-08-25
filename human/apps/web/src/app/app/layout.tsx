import { cookies, headers } from "next/headers";
import { redirect } from "next/navigation";
import type { ReactNode } from "react";

import { AuthenticatedShell } from "../../shell/app-shell";
import { NotificationCenterProvider } from "../../journeys/notifications";
import { selectServerShell } from "../../shell/server";
import { PrivacyModeProvider } from "../../settings/privacy";
import { privacyPrincipalScope } from "../../settings/server";
import { verifiedWebSession } from "../../auth/server-session";

export const dynamic = "force-dynamic";

export default async function AppPlaneLayout({ children }: Readonly<{ children: ReactNode }>) {
  const [requestHeaders, requestCookies] = await Promise.all([headers(), cookies()]);
  const session = await verifiedWebSession(requestHeaders.get("cookie") ?? "");
  if (session === undefined) {
    redirect("/?return_to=%2Fapp");
  }

  return (
    <PrivacyModeProvider principalScope={privacyPrincipalScope(session.principalScope)}>
      <NotificationCenterProvider>
        <AuthenticatedShell initialSelection={selectServerShell(requestHeaders, requestCookies)}>
          {children}
        </AuthenticatedShell>
      </NotificationCenterProvider>
    </PrivacyModeProvider>
  );
}
