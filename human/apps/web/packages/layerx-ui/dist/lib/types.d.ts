import * as React from 'react';

/** One row of money movement (transaction, earning, agent spend…). */
interface MoneyItem {
    id: string;
    title: string;
    subtitle?: string;
    /** Signed amount in major units. */
    amount: number;
    currency?: string;
    status?: string;
    /** Used for month bands + sort. */
    date: Date;
    /** Leading visual: Avatar, IconTile, flag… */
    leading?: React.ReactNode;
}
/** A month band with its rows and subtotal. */
interface MoneyGroup {
    id: string;
    label: string;
    subtotal: number;
    currency?: string;
    items: MoneyItem[];
}
interface NotificationItem {
    id: string;
    title: string;
    body: React.ReactNode;
    /** When it happened — drives recency segments + labels. */
    date: Date;
    icon?: React.ReactNode;
    read?: boolean;
    href?: string;
}
/** Bucket a notification lands in for the recency segments. */
type RecencySegment = "today" | "week" | "month";
declare function recencyOf(date: Date, now?: Date): RecencySegment;

export { type MoneyGroup, type MoneyItem, type NotificationItem, type RecencySegment, recencyOf };
