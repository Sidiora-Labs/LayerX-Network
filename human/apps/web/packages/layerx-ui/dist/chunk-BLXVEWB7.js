"use client";
import {
  recencyOf
} from "./chunk-5ZKGJ4B4.js";
import {
  SegmentedControl
} from "./chunk-DSAF46UG.js";
import {
  IconTile,
  List,
  ListItem
} from "./chunk-EP5CBYSP.js";
import {
  Popover,
  PopoverContent,
  PopoverTrigger
} from "./chunk-XNUMZ4ML.js";
import {
  EmptyState
} from "./chunk-34BAVXSZ.js";
import {
  formatRecency
} from "./chunk-WM3FOCWV.js";
import {
  IconButton
} from "./chunk-4X2DK7Y3.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/notifications.tsx
import * as React from "react";
import { ArrowLeft, Bell } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
var SEGMENTS = [
  { value: "today", label: "Today" },
  { value: "week", label: "This week" },
  { value: "month", label: "This month" }
];
function NotificationRow({
  item,
  onClick
}) {
  return /* @__PURE__ */ jsx(
    ListItem,
    {
      leading: /* @__PURE__ */ jsx(IconTile, { shape: "circle", className: "size-10 [&_svg]:size-4", children: item.icon ?? /* @__PURE__ */ jsx(Bell, {}) }),
      title: /* @__PURE__ */ jsxs("span", { className: "flex items-center gap-2", children: [
        /* @__PURE__ */ jsx("span", { className: "truncate", children: item.title }),
        !item.read && /* @__PURE__ */ jsx("span", { className: "size-2 shrink-0 rounded-full bg-accent", "aria-label": "Unread" })
      ] }),
      subtitle: /* @__PURE__ */ jsx("span", { className: "line-clamp-2 whitespace-normal", children: item.body }),
      trailing: /* @__PURE__ */ jsx("span", { className: "text-xs text-faint-foreground", children: formatRecency(item.date) }),
      onClick: onClick ? () => onClick(item) : void 0,
      className: "items-start"
    }
  );
}
function filterBySegment(items, segment) {
  return items.filter((n) => recencyOf(n.date) === segment);
}
function NotificationsScreen({
  items,
  onBack,
  onItemClick,
  segment: segmentProp,
  onSegmentChange,
  className
}) {
  const [internal, setInternal] = React.useState("today");
  const segment = segmentProp ?? internal;
  const setSegment = onSegmentChange ?? setInternal;
  const shown = filterBySegment(items, segment);
  return /* @__PURE__ */ jsxs("div", { className: cn("flex h-full flex-col bg-background", className), children: [
    /* @__PURE__ */ jsxs("header", { className: "relative flex items-center justify-center border-b border-border bg-surface px-4 pt-[max(0.875rem,env(safe-area-inset-top))] pb-3.5", children: [
      /* @__PURE__ */ jsx(
        IconButton,
        {
          variant: "outline",
          size: "sm",
          onClick: onBack,
          "aria-label": "Back",
          className: "absolute left-4",
          children: /* @__PURE__ */ jsx(ArrowLeft, {})
        }
      ),
      /* @__PURE__ */ jsx("h2", { className: "text-[17px] font-bold text-foreground", children: "Notifications" })
    ] }),
    /* @__PURE__ */ jsx("div", { className: "p-4 pb-2", children: /* @__PURE__ */ jsx(
      SegmentedControl,
      {
        "aria-label": "Recency",
        options: SEGMENTS,
        value: segment,
        onValueChange: (v) => setSegment(v)
      }
    ) }),
    /* @__PURE__ */ jsx("div", { className: "lx-scroll flex-1 overflow-y-auto px-4 pb-[max(1.5rem,env(safe-area-inset-bottom))]", children: shown.length > 0 ? /* @__PURE__ */ jsx(List, { children: shown.map((n) => /* @__PURE__ */ jsx(NotificationRow, { item: n, onClick: onItemClick }, n.id)) }) : /* @__PURE__ */ jsx(
      EmptyState,
      {
        className: "mt-6",
        icon: /* @__PURE__ */ jsx(Bell, {}),
        title: "Nothing here yet",
        description: "You're all caught up for this period."
      }
    ) })
  ] });
}
function BellPopover({
  items,
  onItemClick,
  onViewAll,
  unreadCount
}) {
  const unread = unreadCount ?? items.filter((n) => !n.read).length;
  const recent = [...items].sort((a, b) => b.date.getTime() - a.date.getTime()).slice(0, 5);
  return /* @__PURE__ */ jsxs(Popover, { children: [
    /* @__PURE__ */ jsx(PopoverTrigger, { asChild: true, children: /* @__PURE__ */ jsxs("span", { className: "relative inline-flex", children: [
      /* @__PURE__ */ jsx(IconButton, { variant: "outline", size: "sm", "aria-label": "Notifications", children: /* @__PURE__ */ jsx(Bell, {}) }),
      unread > 0 && /* @__PURE__ */ jsx("span", { className: "pointer-events-none absolute -top-0.5 -right-0.5 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-bold text-destructive-foreground", children: unread })
    ] }) }),
    /* @__PURE__ */ jsxs(PopoverContent, { align: "end", className: "w-[380px] p-0", children: [
      /* @__PURE__ */ jsxs("div", { className: "flex items-center justify-between border-b border-border px-4 py-3", children: [
        /* @__PURE__ */ jsx("span", { className: "text-sm font-bold text-foreground", children: "Notifications" }),
        unread > 0 && /* @__PURE__ */ jsxs("span", { className: "text-xs font-semibold text-muted-foreground", children: [
          unread,
          " unread"
        ] })
      ] }),
      /* @__PURE__ */ jsx("div", { className: "lx-scroll max-h-[360px] overflow-y-auto px-2 py-1", children: recent.length > 0 ? /* @__PURE__ */ jsx(List, { className: "divide-border/50", children: recent.map((n) => /* @__PURE__ */ jsx(NotificationRow, { item: n, onClick: onItemClick }, n.id)) }) : /* @__PURE__ */ jsx("p", { className: "py-8 text-center text-sm text-muted-foreground", children: "You're all caught up." }) }),
      /* @__PURE__ */ jsx("div", { className: "border-t border-border p-2", children: /* @__PURE__ */ jsx(
        "button",
        {
          type: "button",
          onClick: onViewAll,
          className: "flex w-full items-center justify-center rounded-md py-2 text-sm font-semibold text-accent transition-colors hover:bg-accent-soft",
          children: "View all notifications"
        }
      ) })
    ] })
  ] });
}
function NotificationsArchive({
  items,
  onItemClick,
  segment: segmentProp,
  onSegmentChange,
  className
}) {
  const [internal, setInternal] = React.useState("today");
  const segment = segmentProp ?? internal;
  const setSegment = onSegmentChange ?? setInternal;
  const shown = filterBySegment(items, segment);
  return /* @__PURE__ */ jsxs("div", { className: cn("mx-auto flex max-w-[720px] flex-col gap-4", className), children: [
    /* @__PURE__ */ jsxs("div", { className: "flex items-center justify-between gap-4", children: [
      /* @__PURE__ */ jsx("h2", { className: "text-xl font-bold text-foreground", children: "Notifications" }),
      /* @__PURE__ */ jsx(
        SegmentedControl,
        {
          "aria-label": "Recency",
          size: "sm",
          className: "w-[320px]",
          options: SEGMENTS,
          value: segment,
          onValueChange: (v) => setSegment(v)
        }
      )
    ] }),
    /* @__PURE__ */ jsx("div", { className: "rounded-lg border border-border bg-surface px-5 py-2", children: shown.length > 0 ? /* @__PURE__ */ jsx(List, { children: shown.map((n) => /* @__PURE__ */ jsx(NotificationRow, { item: n, onClick: onItemClick }, n.id)) }) : /* @__PURE__ */ jsx(
      EmptyState,
      {
        icon: /* @__PURE__ */ jsx(Bell, {}),
        title: "Nothing here yet",
        description: "You're all caught up for this period.",
        className: "my-4"
      }
    ) })
  ] });
}

export {
  NotificationsScreen,
  BellPopover,
  NotificationsArchive
};
//# sourceMappingURL=chunk-BLXVEWB7.js.map