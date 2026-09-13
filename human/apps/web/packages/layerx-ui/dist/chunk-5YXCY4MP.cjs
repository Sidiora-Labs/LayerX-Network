"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";


var _chunkUF45QVNGcjs = require('./chunk-UF45QVNG.cjs');


var _chunk5ISTU3ZIcjs = require('./chunk-5ISTU3ZI.cjs');




var _chunkZY7BNJKWcjs = require('./chunk-ZY7BNJKW.cjs');




var _chunk7SUKWFMOcjs = require('./chunk-7SUKWFMO.cjs');


var _chunkMEQICWTKcjs = require('./chunk-MEQICWTK.cjs');


var _chunkW6TE4RURcjs = require('./chunk-W6TE4RUR.cjs');


var _chunkRA3A4XJ2cjs = require('./chunk-RA3A4XJ2.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/notifications.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
var SEGMENTS = [
  { value: "today", label: "Today" },
  { value: "week", label: "This week" },
  { value: "month", label: "This month" }
];
function NotificationRow({
  item,
  onClick
}) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
    _chunkZY7BNJKWcjs.ListItem,
    {
      leading: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkZY7BNJKWcjs.IconTile, { shape: "circle", className: "size-10 [&_svg]:size-4", children: _nullishCoalesce(item.icon, () => ( /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Bell, {}))) }),
      title: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "flex items-center gap-2", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "truncate", children: item.title }),
        !item.read && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "size-2 shrink-0 rounded-full bg-accent", "aria-label": "Unread" })
      ] }),
      subtitle: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "line-clamp-2 whitespace-normal", children: item.body }),
      trailing: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-xs text-faint-foreground", children: _chunkW6TE4RURcjs.formatRecency.call(void 0, item.date) }),
      onClick: onClick ? () => onClick(item) : void 0,
      className: "items-start"
    }
  );
}
function filterBySegment(items, segment) {
  return items.filter((n) => _chunkUF45QVNGcjs.recencyOf.call(void 0, n.date) === segment);
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
  const segment = _nullishCoalesce(segmentProp, () => ( internal));
  const setSegment = _nullishCoalesce(onSegmentChange, () => ( setInternal));
  const shown = filterBySegment(items, segment);
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex h-full flex-col bg-background", className), children: [
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "header", { className: "relative flex items-center justify-center border-b border-border bg-surface px-4 pt-[max(0.875rem,env(safe-area-inset-top))] pb-3.5", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
        _chunkRA3A4XJ2cjs.IconButton,
        {
          variant: "outline",
          size: "sm",
          onClick: onBack,
          "aria-label": "Back",
          className: "absolute left-4",
          children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ArrowLeft, {})
        }
      ),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h2", { className: "text-[17px] font-bold text-foreground", children: "Notifications" })
    ] }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "p-4 pb-2", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
      _chunk5ISTU3ZIcjs.SegmentedControl,
      {
        "aria-label": "Recency",
        options: SEGMENTS,
        value: segment,
        onValueChange: (v) => setSegment(v)
      }
    ) }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "lx-scroll flex-1 overflow-y-auto px-4 pb-[max(1.5rem,env(safe-area-inset-bottom))]", children: shown.length > 0 ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkZY7BNJKWcjs.List, { children: shown.map((n) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, NotificationRow, { item: n, onClick: onItemClick }, n.id)) }) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
      _chunkMEQICWTKcjs.EmptyState,
      {
        className: "mt-6",
        icon: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Bell, {}),
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
  const unread = _nullishCoalesce(unreadCount, () => ( items.filter((n) => !n.read).length));
  const recent = [...items].sort((a, b) => b.date.getTime() - a.date.getTime()).slice(0, 5);
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunk7SUKWFMOcjs.Popover, { children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunk7SUKWFMOcjs.PopoverTrigger, { asChild: true, children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "relative inline-flex", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkRA3A4XJ2cjs.IconButton, { variant: "outline", size: "sm", "aria-label": "Notifications", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Bell, {}) }),
      unread > 0 && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "pointer-events-none absolute -top-0.5 -right-0.5 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-bold text-destructive-foreground", children: unread })
    ] }) }),
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunk7SUKWFMOcjs.PopoverContent, { align: "end", className: "w-[380px] p-0", children: [
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-center justify-between border-b border-border px-4 py-3", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-sm font-bold text-foreground", children: "Notifications" }),
        unread > 0 && /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "text-xs font-semibold text-muted-foreground", children: [
          unread,
          " unread"
        ] })
      ] }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "lx-scroll max-h-[360px] overflow-y-auto px-2 py-1", children: recent.length > 0 ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkZY7BNJKWcjs.List, { className: "divide-border/50", children: recent.map((n) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, NotificationRow, { item: n, onClick: onItemClick }, n.id)) }) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "p", { className: "py-8 text-center text-sm text-muted-foreground", children: "You're all caught up." }) }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "border-t border-border p-2", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
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
  const segment = _nullishCoalesce(segmentProp, () => ( internal));
  const setSegment = _nullishCoalesce(onSegmentChange, () => ( setInternal));
  const shown = filterBySegment(items, segment);
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "mx-auto flex max-w-[720px] flex-col gap-4", className), children: [
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-center justify-between gap-4", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h2", { className: "text-xl font-bold text-foreground", children: "Notifications" }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
        _chunk5ISTU3ZIcjs.SegmentedControl,
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
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "rounded-lg border border-border bg-surface px-5 py-2", children: shown.length > 0 ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkZY7BNJKWcjs.List, { children: shown.map((n) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, NotificationRow, { item: n, onClick: onItemClick }, n.id)) }) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
      _chunkMEQICWTKcjs.EmptyState,
      {
        icon: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Bell, {}),
        title: "Nothing here yet",
        description: "You're all caught up for this period.",
        className: "my-4"
      }
    ) })
  ] });
}





exports.NotificationsScreen = NotificationsScreen; exports.BellPopover = BellPopover; exports.NotificationsArchive = NotificationsArchive;
//# sourceMappingURL=chunk-5YXCY4MP.cjs.map