"use client";

import { Badge } from "@layerx/ui/components/badge";
import { Button } from "@layerx/ui/components/button";
import { Card } from "@layerx/ui/components/card";
import { List, ListItem } from "@layerx/ui/components/list";
import { useMemo, useState, type ReactNode } from "react";

export interface ActivityFeedRow {
  readonly id: string;
  readonly title: string;
  readonly subtitle: string;
  readonly occurredAt: Date;
  readonly sortAmount: number;
  readonly amount: ReactNode;
  readonly status: ReactNode;
}

export interface ActivityFeedGroup {
  readonly id: string;
  readonly label: string;
  readonly subtotal: ReactNode;
  readonly rows: readonly ActivityFeedRow[];
}

export interface ActivityFeedProps {
  readonly groups: readonly ActivityFeedGroup[];
  readonly onSelect: (entryId: string) => void;
  readonly dateLabel: (date: Date) => string;
  readonly columns: Readonly<{
    activity: string;
    date: string;
    status: string;
    amount: string;
  }>;
  readonly amountSortEnabled: boolean;
}

export function MobileActivityFeed({ groups, onSelect, dateLabel }: ActivityFeedProps) {
  return (
    <div className="flex flex-col gap-6">
      {groups.map((group) => (
        <section key={group.id} aria-labelledby={`activity-month-${group.id}`}>
          <div className="flex items-baseline justify-between gap-4 pb-2">
            <h2 id={`activity-month-${group.id}`} className="text-sm font-bold text-muted-foreground">
              {group.label}
            </h2>
            <span className="text-sm font-semibold text-foreground-secondary">{group.subtotal}</span>
          </div>
          <List>
            {group.rows.map((row) => (
              <ListItem
                key={row.id}
                title={row.title}
                subtitle={<span className="flex flex-col gap-1"><span>{row.subtitle}</span>{row.status}</span>}
                trailing={row.amount}
                trailingCaption={dateLabel(row.occurredAt)}
                onClick={() => { onSelect(row.id); }}
              />
            ))}
          </List>
        </section>
      ))}
    </div>
  );
}

type ActivitySort = "activity" | "amount" | "date";
type SortDirection = "ascending" | "descending";

function sortRows(
  rows: readonly ActivityFeedRow[],
  key: ActivitySort,
  direction: SortDirection,
): readonly ActivityFeedRow[] {
  const multiplier = direction === "ascending" ? 1 : -1;
  return [...rows].sort((left, right) => {
    if (key === "activity") {
      return multiplier * left.title.localeCompare(right.title, "en-US");
    }
    if (key === "amount") {
      return multiplier * (left.sortAmount - right.sortAmount);
    }
    return multiplier * (left.occurredAt.getTime() - right.occurredAt.getTime());
  });
}

export function DesktopActivityFeed({ groups, onSelect, dateLabel, columns, amountSortEnabled }: ActivityFeedProps) {
  const [sort, setSort] = useState<ActivitySort>("date");
  const [direction, setDirection] = useState<SortDirection>("descending");
  const activeSort = !amountSortEnabled && sort === "amount" ? "date" : sort;
  const sorted = useMemo(
    () => groups.map((group) => ({ ...group, rows: sortRows(group.rows, activeSort, direction) })),
    [activeSort, direction, groups],
  );
  const changeSort = (next: ActivitySort) => {
    if (next === sort) {
      setDirection((current) => current === "ascending" ? "descending" : "ascending");
      return;
    }
    setSort(next);
    setDirection(next === "activity" ? "ascending" : "descending");
  };
  const sortHeader = (key: ActivitySort, label: string) => (
    <Button
      type="button"
      variant="link"
      size="sm"
      onClick={() => { changeSort(key); }}
    >
      {label}
      {activeSort === key ? (direction === "ascending" ? " ↑" : " ↓") : null}
    </Button>
  );

  return (
    <Card elevation="outline" padding="none" className="overflow-x-auto">
      <table className="w-full min-w-[48rem] border-collapse text-left text-sm">
        <thead className="bg-surface-sunken text-foreground-secondary">
          <tr>
            <th scope="col" aria-sort={activeSort === "activity" ? direction : "none"} className="border-b border-border px-4 py-2">{sortHeader("activity", columns.activity)}</th>
            <th scope="col" aria-sort={activeSort === "date" ? direction : "none"} className="border-b border-border px-4 py-2">{sortHeader("date", columns.date)}</th>
            <th scope="col" className="border-b border-border px-4 py-2 font-semibold">{columns.status}</th>
            <th scope="col" aria-sort={activeSort === "amount" ? direction : "none"} className="border-b border-border px-4 py-2 text-right">
              {amountSortEnabled ? sortHeader("amount", columns.amount) : <span className="font-semibold">{columns.amount}</span>}
            </th>
          </tr>
        </thead>
        {sorted.map((group) => (
          <tbody key={group.id} className="divide-y divide-border">
            <tr className="bg-surface-sunken/70">
              <th scope="rowgroup" colSpan={3} className="px-4 py-2 font-bold text-muted-foreground">
                {group.label}
              </th>
              <td className="px-4 py-2 text-right font-semibold text-foreground-secondary">
                {group.subtotal}
              </td>
            </tr>
            {group.rows.map((row) => (
              <tr key={row.id} className="align-middle">
                <td className="px-4 py-3">
                  <Button type="button" variant="link" size="sm" onClick={() => { onSelect(row.id); }}>
                    <span className="flex flex-col items-start gap-1 text-left">
                      <span>{row.title}</span>
                      <span className="font-normal text-muted-foreground">{row.subtitle}</span>
                    </span>
                  </Button>
                </td>
                <td className="whitespace-nowrap px-4 py-3 tabular-nums text-muted-foreground">
                  {dateLabel(row.occurredAt)}
                </td>
                <td className="px-4 py-3">{row.status}</td>
                <td className="px-4 py-3 text-right">{row.amount}</td>
              </tr>
            ))}
          </tbody>
        ))}
      </table>
    </Card>
  );
}

export function ActivityEvidenceBadge({ children }: Readonly<{ children: ReactNode }>) {
  return <Badge variant="neutral">{children}</Badge>;
}
