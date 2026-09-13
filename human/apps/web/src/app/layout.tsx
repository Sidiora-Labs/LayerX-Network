import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";

import { copyEntry } from "../../copy/catalog";
import { WebVitalsRum } from "../perf/web-vitals-rum";
import { ApplicationStateBoundary } from "../states/application";

import "./globals.css";

export const metadata: Metadata = {
  title: copyEntry("application.name").message,
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  viewportFit: "cover",
  interactiveWidget: "resizes-content",
};

export default function RootLayout({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <html lang="en">
      <body>
        <WebVitalsRum />
        <ApplicationStateBoundary>{children}</ApplicationStateBoundary>
      </body>
    </html>
  );
}
