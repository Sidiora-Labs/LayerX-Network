import { Badge, Card, Input } from "@layerx/ui";
import { cn } from "@layerx/ui/cn";
import Link from "next/link";
import type { ReactNode, TextareaHTMLAttributes } from "react";

import { KitButton } from "./control";

export function ExplorerNavigation({
  label,
  items,
}: Readonly<{ label: string; items: readonly Readonly<{ href: string; label: string }>[] }>) {
  return (
    <nav aria-label={label} className="flex flex-wrap gap-2">
      {items.map((item) => (
        <Link
          key={item.href}
          href={item.href}
          className="inline-flex min-h-11 items-center rounded-full border border-border-strong bg-surface px-4 text-sm font-semibold text-foreground hover:bg-surface-sunken"
        >
          {item.label}
        </Link>
      ))}
    </nav>
  );
}

export function ExplorerPanel({
  title,
  children,
}: Readonly<{ title: ReactNode; children: ReactNode }>) {
  return (
    <Card elevation="outline" className="flex flex-col gap-3">
      <h2 className="text-lg font-bold text-foreground">{title}</h2>
      {children}
    </Card>
  );
}

export function ExplorerTable({
  caption,
  columns,
  rows,
}: Readonly<{
  caption: string;
  columns: readonly string[];
  rows: readonly Readonly<{ id: string; cells: readonly ReactNode[] }>[];
}>) {
  return (
    <div className="overflow-x-auto rounded-md border border-border">
      <table className="w-full min-w-[44rem] border-collapse text-left text-sm">
        <caption className="sr-only">{caption}</caption>
        <thead className="bg-surface-sunken text-foreground-secondary">
          <tr>
            {columns.map((column) => (
              <th key={column} scope="col" className="border-b border-border px-3 py-2 font-semibold">
                {column}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-border">
          {rows.map((row) => (
            <tr key={row.id} className="align-top">
              {row.cells.map((cell, index) => (
                <td key={`${row.id}-${String(index)}`} className="px-3 py-3 text-foreground">
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function ExplorerLink({ href, children }: Readonly<{ href: string; children: ReactNode }>) {
  return <Link href={href} className="font-semibold text-accent hover:underline">{children}</Link>;
}

export function ExplorerVerificationBadge({
  label,
  unverified = false,
}: Readonly<{ label: string; unverified?: boolean }>) {
  return <Badge variant={unverified ? "warning" : "success"} data-verification={unverified ? "unverified" : "verified"}>{label}</Badge>;
}

export function ExplorerFreshness({
  title,
  description,
  current,
}: Readonly<{ title: string; description: string; current: boolean }>) {
  return (
    <div
      role="status"
      className={cn(
        "rounded-md border px-4 py-3 text-sm",
        current
          ? "border-success bg-success-soft text-success"
          : "border-warning bg-warning-soft text-warning",
      )}
    >
      <p className="font-semibold">{title}</p>
      <p>{description}</p>
    </div>
  );
}

export function ExplorerLookupForm({
  action,
  kind,
  label,
  placeholder,
  submitLabel,
}: Readonly<{
  action: string;
  kind: "receipt" | "account" | "checkpoint" | "batch" | "program";
  label: string;
  placeholder: string;
  submitLabel: string;
}>) {
  return (
    <form action={action} method="get" className="flex flex-col gap-2 sm:flex-row sm:items-end">
      <label className="flex flex-1 flex-col gap-1 text-sm font-semibold text-foreground">
        {label}
        <Input name="identifier" placeholder={placeholder} required autoComplete="off" spellCheck={false} />
      </label>
      <input type="hidden" name="kind" value={kind} />
      <KitButton type="submit">{submitLabel}</KitButton>
    </form>
  );
}

export function ExplorerEvidenceInput({
  className,
  ...props
}: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      {...props}
      className={cn(
        "min-h-40 w-full resize-y rounded-md border border-border bg-surface p-3 font-mono text-sm text-foreground outline-none focus:border-accent focus:ring-2 focus:ring-accent/20",
        className,
      )}
    />
  );
}
