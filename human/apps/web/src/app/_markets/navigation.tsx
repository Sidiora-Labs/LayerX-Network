import { ExplorerNavigation } from "../../kit/explorer";

export const MARKET_ROUTES = [
  { href: "/launchpad", label: "Launchpad" },
  { href: "/exchange", label: "Exchange" },
  { href: "/bridge", label: "Bridge" },
] as const;

export function MarketNavigation({ account }: Readonly<{ account: string | undefined }>) {
  const suffix = account === undefined ? "" : `?account=${account}`;
  return (
    <ExplorerNavigation
      label="Markets"
      items={[
        { href: "/", label: "Home" },
        ...MARKET_ROUTES.map((route) => ({ href: `${route.href}${suffix}`, label: route.label })),
      ]}
    />
  );
}
