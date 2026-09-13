"use client";

import { usePathname, useRouter } from "next/navigation";
import {
  createContext,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { copyEntry } from "../../copy/runtime";
import { DesktopNavigation, MobileNavigation, type NavigationProps } from "../kit/pattern-navigation";
import { DesktopNotifications } from "../kit/pattern-notifications";
import {
  notificationItems,
  useNotificationCenter,
} from "../journeys/notifications";
import {
  SHELL_HINT_COOKIE,
  ShellSelector,
  type PointerCapability,
  type ShellSelection,
} from "./selector";

const NAVIGATION = [
  { id: "home", label: copyEntry("navigation.home").message },
  { id: "agents", label: copyEntry("navigation.agents").message },
  { id: "activity", label: copyEntry("navigation.activity").message },
  { id: "more", label: copyEntry("navigation.more").message },
] as const satisfies readonly NavigationProps["nav"][number][];

const AuthenticatedShellContext = createContext<ShellSelection | undefined>(undefined);

const DESTINATIONS: Readonly<Record<(typeof NAVIGATION)[number]["id"], string>> = {
  home: "/app",
  agents: "/app/agents",
  activity: "/app/activity",
  more: "/app/settings",
};

export function useShellSelection(): ShellSelection {
  const selection = useContext(AuthenticatedShellContext);
  if (selection === undefined) {
    throw new Error("useShellSelection requires the authenticated shell");
  }
  return selection;
}

function pointerCapability(): PointerCapability {
  if (window.matchMedia("(pointer: coarse)").matches) {
    return "coarse";
  }
  if (window.matchMedia("(pointer: fine)").matches) {
    return "fine";
  }
  return "none";
}

function clientHints() {
  return { viewportWidth: window.innerWidth, pointer: pointerCapability() } as const;
}

function persistHints(selection: ShellSelection): void {
  if (selection.viewportWidth === undefined) {
    return;
  }
  document.cookie = `${SHELL_HINT_COOKIE}=v1.${String(selection.viewportWidth)}.${selection.pointer}; Path=/; Max-Age=2592000; SameSite=Lax; Secure`;
}

function activeNavigation(pathname: string): (typeof NAVIGATION)[number]["id"] {
  if (pathname.startsWith("/app/agents")) {
    return "agents";
  }
  if (
    pathname.startsWith("/app/activity")
    || pathname.startsWith("/app/approvals")
    || pathname.startsWith("/app/notifications")
  ) {
    return "activity";
  }
  if (pathname.startsWith("/app/settings") || pathname.startsWith("/app/security")) {
    return "more";
  }
  return "home";
}

export function AuthenticatedShell({
  initialSelection,
  children,
}: Readonly<{ initialSelection: ShellSelection; children: ReactNode }>) {
  const [selection, setSelection] = useState(initialSelection);
  const [corrected, setCorrected] = useState(false);
  const frame = useRef<number | undefined>(undefined);
  const pathname = usePathname();
  const router = useRouter();
  const notificationCenter = useNotificationCenter();
  const Navigation = selection.shell === "mobile" ? MobileNavigation : DesktopNavigation;
  const approvalCount = notificationCenter.state.approvalCount;
  const nav = useMemo(() => NAVIGATION.map((item) => item.id === "activity" && approvalCount > 0
    ? { ...item, badge: approvalCount }
    : item), [approvalCount]);
  const notifications = notificationCenter.state.status === "ready"
    ? notificationItems(notificationCenter.state.notifications)
    : [];

  useLayoutEffect(() => {
    const applyClientSelection = () => {
      const confirmation = ShellSelector.confirm(initialSelection, clientHints());
      setSelection((current) => {
        if (
          current.shell === confirmation.selection.shell &&
          current.pointer === confirmation.selection.pointer &&
          current.viewportWidth === confirmation.selection.viewportWidth &&
          current.touchTargets === confirmation.selection.touchTargets
        ) {
          return current;
        }
        return confirmation.selection;
      });
      setCorrected(confirmation.correction === "pre-paint");
      persistHints(confirmation.selection);
    };
    const scheduleClientSelection = () => {
      if (frame.current !== undefined) {
        cancelAnimationFrame(frame.current);
      }
      frame.current = requestAnimationFrame(applyClientSelection);
    };
    const coarsePointer = window.matchMedia("(pointer: coarse)");
    const finePointer = window.matchMedia("(pointer: fine)");

    applyClientSelection();
    window.addEventListener("resize", scheduleClientSelection, { passive: true });
    coarsePointer.addEventListener("change", scheduleClientSelection);
    finePointer.addEventListener("change", scheduleClientSelection);
    return () => {
      window.removeEventListener("resize", scheduleClientSelection);
      coarsePointer.removeEventListener("change", scheduleClientSelection);
      finePointer.removeEventListener("change", scheduleClientSelection);
      if (frame.current !== undefined) {
        cancelAnimationFrame(frame.current);
      }
    };
  }, [initialSelection]);

  const navigate = (id: string) => {
    if (!Object.hasOwn(DESTINATIONS, id)) {
      return;
    }
    const destination = DESTINATIONS[id as keyof typeof DESTINATIONS];
    router.push(ShellSelector.resolveDeepLink(destination, selection.shell).href);
  };

  const openNotification = async (id: string) => {
    if (notificationCenter.state.status !== "ready") {
      router.push("/app/notifications");
      return;
    }
    const notification = notificationCenter.state.notifications.find(
      (candidate) => candidate.source.notification_id === id,
    );
    if (notification === undefined) {
      router.push("/app/notifications");
      return;
    }
    try {
      const landing = await notificationCenter.open(notification);
      router.push(landing.href);
    } catch {
      router.push("/app/notifications");
    }
  };

  return (
    <AuthenticatedShellContext.Provider value={selection}>
      <div
        data-shell={selection.shell}
        data-server-shell={initialSelection.shell}
        data-shell-corrected={corrected ? "true" : "false"}
        data-touch-targets={selection.touchTargets ? "true" : "false"}
      >
        <Navigation
          nav={nav}
          activeNav={activeNavigation(pathname)}
          onNavigate={navigate}
          onPrimaryAction={() => {
            router.push("/app");
          }}
          onNotifications={() => { router.push("/app/notifications"); }}
          notificationCount={notificationCenter.state.unreadCount}
          {...(selection.shell === "desktop" ? {
            notificationControl: (
              <DesktopNotifications
                view="popover"
                items={notifications}
                unreadCount={notificationCenter.state.unreadCount}
                onItemClick={(item) => { void openNotification(item.id); }}
                onViewAll={() => { router.push("/app/notifications"); }}
              />
            ),
          } : {})}
          onExplorer={() => {
            router.push("/explorer");
          }}
          onSettings={() => {
            router.push("/app/settings");
          }}
          title={copyEntry(`navigation.${activeNavigation(pathname)}`).message}
        >
          {children}
        </Navigation>
      </div>
    </AuthenticatedShellContext.Provider>
  );
}

export function useAuthenticatedShell(): ShellSelection {
  const selection = useContext(AuthenticatedShellContext);
  if (selection === undefined) {
    throw new Error("AuthenticatedShell is required on authenticated surfaces");
  }
  return selection;
}
