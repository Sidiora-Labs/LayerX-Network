import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "LayerX Developer",
  description: "API keys, usage, protocol requests, webhook delivery and verified receipts",
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return <html lang="en"><body>{children}</body></html>;
}
