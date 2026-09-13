"use client";
import {
  Badge
} from "./chunk-KTG64LT6.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/bank-card.tsx
import * as React from "react";
import { jsx, jsxs } from "react/jsx-runtime";
function BankCard({ data, className }) {
  const dark = data.theme === "dark";
  return /* @__PURE__ */ jsxs(
    "div",
    {
      className: cn(
        "relative flex aspect-[8/5] w-full flex-col justify-between overflow-hidden rounded-lg p-5 shadow-card",
        dark ? "bg-[#101418] text-white" : "bg-[linear-gradient(135deg,#eef3fa_0%,#e2eaf5_55%,#dbe5f2_100%)] text-foreground",
        className
      ),
      children: [
        /* @__PURE__ */ jsx(
          "div",
          {
            "aria-hidden": true,
            className: cn(
              "pointer-events-none absolute inset-0",
              dark ? "bg-[radial-gradient(120%_90%_at_80%_0%,rgb(255_255_255/0.08),transparent_60%)]" : "bg-[radial-gradient(120%_90%_at_80%_0%,rgb(255_255_255/0.7),transparent_60%)]"
            )
          }
        ),
        /* @__PURE__ */ jsxs("div", { className: "relative flex items-start justify-between gap-3", children: [
          /* @__PURE__ */ jsx("span", { className: "text-[13px] font-bold tracking-[0.12em] uppercase", children: data.holder }),
          data.status && /* @__PURE__ */ jsx(Badge, { variant: data.status.tone ?? "success", size: "sm", className: "bg-surface/80", children: data.status.label })
        ] }),
        /* @__PURE__ */ jsxs("div", { className: "relative flex items-end justify-between gap-4", children: [
          /* @__PURE__ */ jsxs("div", { className: "flex flex-col gap-1", children: [
            /* @__PURE__ */ jsx("span", { className: cn("text-xs", dark ? "text-white/60" : "text-muted-foreground"), children: data.kind ?? "Virtual card" }),
            /* @__PURE__ */ jsx("span", { className: "text-[15px] font-semibold tracking-[0.06em] tabular-nums", children: data.number })
          ] }),
          /* @__PURE__ */ jsxs("div", { className: "flex flex-col items-end gap-1", children: [
            /* @__PURE__ */ jsx("span", { className: cn("text-xs", dark ? "text-white/60" : "text-muted-foreground"), children: data.balance ? data.balanceLabel ?? "Balance" : "Expiry" }),
            /* @__PURE__ */ jsx("span", { className: "text-[15px] font-semibold tabular-nums", children: data.balance ?? data.expiry })
          ] })
        ] }),
        /* @__PURE__ */ jsxs("div", { className: "relative flex items-center justify-between", children: [
          /* @__PURE__ */ jsx("span", { className: cn("text-lg font-black tracking-tight", dark ? "text-white" : "text-foreground"), children: "\u224B" }),
          /* @__PURE__ */ jsx("span", { className: "text-sm font-black tracking-[0.08em] uppercase italic", children: data.brand ?? "VISA" })
        ] })
      ]
    }
  );
}
function CardCarousel({
  cards,
  renderCard,
  className
}) {
  const [active, setActive] = React.useState(0);
  const trackRef = React.useRef(null);
  const onScroll = () => {
    const el = trackRef.current;
    if (!el) return;
    const i = Math.round(el.scrollLeft / el.clientWidth);
    setActive(Math.max(0, Math.min(cards.length - 1, i)));
  };
  return /* @__PURE__ */ jsxs("div", { className: cn("flex flex-col gap-3", className), children: [
    /* @__PURE__ */ jsx(
      "div",
      {
        ref: trackRef,
        onScroll,
        className: "lx-scroll flex snap-x snap-mandatory gap-3 overflow-x-auto",
        children: cards.map((c, i) => /* @__PURE__ */ jsx("div", { className: "w-full shrink-0 snap-center", children: renderCard ? renderCard(c, i) : /* @__PURE__ */ jsx(BankCard, { data: c }) }, i))
      }
    ),
    cards.length > 1 && /* @__PURE__ */ jsx("div", { className: "flex items-center justify-center gap-1.5", "aria-hidden": true, children: cards.map((_, i) => /* @__PURE__ */ jsx(
      "span",
      {
        className: cn(
          "size-1.5 rounded-full transition-colors",
          i === active ? "bg-foreground" : "bg-border-strong"
        )
      },
      i
    )) })
  ] });
}

export {
  BankCard,
  CardCarousel
};
//# sourceMappingURL=chunk-SK7OEW2S.js.map