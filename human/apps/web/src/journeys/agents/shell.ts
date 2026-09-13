"use client";

import { useShellSelection } from "../../shell/app-shell.tsx";
import type { AgentsShell } from "./model.ts";

export function useAgentsShell(): AgentsShell {
  return useShellSelection().shell;
}
