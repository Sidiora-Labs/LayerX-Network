import { Card } from "@layerx/ui/components/card";
import { Skeleton } from "@layerx/ui/components/feedback";
import type { ReactNode } from "react";

export function PerformanceLoadingCard({
  plane,
  label,
}: Readonly<{ plane: "app" | "explorer"; label: ReactNode }>) {
  return (
    <Card className="min-h-48">
      <section aria-busy="true" aria-live="polite" data-honest-progress={plane} role="status">
        <p>{label}</p>
        <Skeleton className="mt-4 h-8 w-48" />
        <Skeleton className="mt-3 h-5 w-full" />
        <Skeleton className="mt-2 h-5 w-3/4" />
        <Skeleton className="mt-6 h-11 w-32 rounded-full" />
      </section>
    </Card>
  );
}
