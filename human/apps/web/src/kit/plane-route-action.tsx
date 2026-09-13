"use client";

import { useRouter } from "next/navigation";
import { useTransition, type ReactNode } from "react";

import { copyEntry } from "../../copy/runtime";
import { KitButton } from "./control";
import { PerformanceLoadingCard } from "./performance";

export function PlaneRouteAction({
  destination,
  children,
}: Readonly<{ destination: "/app" | "/explorer"; children: ReactNode }>) {
  const router = useRouter();
  const [pending, startTransition] = useTransition();
  return (
    <>
      <KitButton loading={pending} onClick={() => {
        startTransition(() => { router.push(destination); });
      }}>{children}</KitButton>
      {pending ? (
        <PerformanceLoadingCard
          plane={destination === "/app" ? "app" : "explorer"}
          label={copyEntry("status.getting_ready").message}
        />
      ) : null}
    </>
  );
}
