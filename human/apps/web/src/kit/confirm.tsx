"use client";

import { Input } from "@layerx/ui/components/input";
import { ResponsiveDialog } from "@layerx/ui/components/responsive-dialog";
import { type Platform } from "@layerx/ui/lib/platform";
import type { ReactNode } from "react";

import { copyEntry } from "../../copy/runtime.ts";
import { formatCopy } from "../../copy/format.ts";
import { KitButton } from "./control";
import { confirmationVariant, typedConfirmationReady } from "./model";

interface ConfirmationBase {
  readonly children?: ReactNode;
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
  readonly title: ReactNode;
  readonly consequence: ReactNode;
  readonly confirmLabel: ReactNode;
  readonly cancelLabel?: ReactNode;
  readonly onConfirm: () => void;
  readonly loading?: boolean;
  readonly icon?: ReactNode;
  readonly portalContainer?: HTMLElement | null;
}

export interface TypedConfirmation {
  readonly expectedValue: string;
  readonly value: string;
  readonly onValueChange: (value: string) => void;
  readonly label?: string;
}

export type ConfirmationProps =
  | (ConfirmationBase & Readonly<{ kind: "reversible"; typedConfirmation?: never }>)
  | (ConfirmationBase & Readonly<{ kind: "destructive"; typedConfirmation?: never }>)
  | (ConfirmationBase & Readonly<{ kind: "irreversible"; typedConfirmation: TypedConfirmation }>);

function Confirmation({ platform, ...props }: ConfirmationProps & Readonly<{ platform: Platform }>) {
  const typed = props.kind === "irreversible" ? props.typedConfirmation : undefined;
  const ready = typedConfirmationReady(props.kind, typed);
  const disabledReason = typed === undefined
    ? undefined
    : formatCopy("confirmation.type_exact", { expectedValue: typed.expectedValue });

  return (
    <ResponsiveDialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={props.title}
      description={props.consequence}
      platform={platform}
      {...(props.portalContainer === undefined ? {} : { portalContainer: props.portalContainer })}
      footer={
        <>
          <KitButton
            variant="secondary"
            size="lg"
            fullWidth={platform === "mobile"}
            onClick={() => { props.onOpenChange(false); }}
          >
            {props.cancelLabel ?? copyEntry("action.cancel").message}
          </KitButton>
          {ready ? (
            <KitButton
              variant={confirmationVariant(props.kind)}
              size="lg"
              fullWidth={platform === "mobile"}
              {...(props.loading === undefined ? {} : { loading: props.loading })}
              onClick={props.onConfirm}
            >
              {props.confirmLabel}
            </KitButton>
          ) : (
            <KitButton
              variant={confirmationVariant(props.kind)}
              size="lg"
              fullWidth={platform === "mobile"}
              disabled
              disabledReason={disabledReason ?? copyEntry("confirmation.incomplete").message}
            >
              {props.confirmLabel}
            </KitButton>
          )}
        </>
      }
    >
      {props.children}
      {typed === undefined ? null : (
        <label className="flex flex-col gap-2 text-sm font-semibold text-foreground">
          {typed.label ?? formatCopy("confirmation.type_to_confirm", { expectedValue: typed.expectedValue })}
          <Input
            value={typed.value}
            onChange={(event) => { typed.onValueChange(event.target.value); }}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
      )}
    </ResponsiveDialog>
  );
}

export function MobileConfirmation(props: ConfirmationProps) {
  return <Confirmation {...props} platform="mobile" />;
}

export function DesktopConfirmation(props: ConfirmationProps) {
  return <Confirmation {...props} platform="desktop" />;
}
