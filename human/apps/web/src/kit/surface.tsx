import { Card } from "@layerx/ui/components/card";
import { EmptyState } from "@layerx/ui/components/empty-state";
import { Skeleton, SkeletonRow } from "@layerx/ui/components/feedback";
import { cn } from "@layerx/ui/cn";
import type { ReactNode } from "react";

export function ScreenCard({
  title,
  description,
  children,
  landmark = "main",
  dataApplication,
}: Readonly<{
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  landmark?: "main" | "section";
  dataApplication?: string;
}>) {
  const Landmark = landmark;
  return (
    <Card>
      <Landmark data-application={dataApplication} className="flex flex-col gap-2">
        <h1 className="text-xl font-bold text-foreground">{title}</h1>
        {description === undefined ? null : (
          <p className="text-foreground-secondary">{description}</p>
        )}
        {children}
      </Landmark>
    </Card>
  );
}

export type StateTone = "neutral" | "warning" | "danger" | "success";

const STATE_TONE_CLASS: Readonly<Record<StateTone, string>> = {
  neutral: "border-border bg-surface text-foreground",
  warning: "border-warning bg-warning-soft text-warning",
  danger: "border-destructive bg-destructive-soft text-destructive",
  success: "border-success bg-success-soft text-success",
};

export function StateFrame({
  title,
  description,
  children,
  tone = "neutral",
  busy = false,
  role,
}: Readonly<{
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  tone?: StateTone;
  busy?: boolean;
  role?: "alert" | "status";
}>) {
  return (
    <Card className={cn("min-h-72 border", STATE_TONE_CLASS[tone])}>
      <section
        aria-busy={busy || undefined}
        {...(role === undefined ? {} : { role })}
        className="flex min-h-64 flex-col justify-center gap-4"
      >
        <div className="flex flex-col gap-2">
          <h1 className="text-xl font-bold">{title}</h1>
          {description === undefined ? null : <p className="text-sm leading-relaxed">{description}</p>}
        </div>
        {children}
      </section>
    </Card>
  );
}

export function StateSkeleton({ rows = 3 }: Readonly<{ rows?: number }>) {
  return (
    <div aria-hidden="true" className="flex flex-col gap-3">
      <Skeleton className="h-5 w-2/5 motion-reduce:animate-none" />
      <Skeleton className="h-4 w-3/5 motion-reduce:animate-none" />
      <div className="mt-2 divide-y divide-border">
        {Array.from({ length: rows }, (_, index) => <SkeletonRow key={index} />)}
      </div>
    </div>
  );
}

export function StateEmpty({
  title,
  description,
  action,
}: Readonly<{ title: ReactNode; description?: ReactNode; action?: ReactNode }>) {
  return <EmptyState title={title} description={description} action={action} className="min-h-64" />;
}

export function InlineNotice({
  children,
  tone = "neutral",
  role = "status",
}: Readonly<{ children: ReactNode; tone?: StateTone; role?: "alert" | "status" }>) {
  return (
    <div role={role} className={cn("border-b px-4 py-2 text-sm font-medium", STATE_TONE_CLASS[tone])}>
      {children}
    </div>
  );
}
