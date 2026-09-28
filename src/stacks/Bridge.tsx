import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { ForwardState, Stack, StackStatus } from "../lib/types";
import { useStore } from "../state/store";
import { Button, cx } from "../ui/primitives";
import type { ChipTone } from "../ui/primitives";

/** The bridge: this computer's localhost ports handed to the machine over
 * ssh. One of four states, in the order a user would ask about them. */
export type BridgeState = "off" | "waiting" | "connecting" | "connected";

export interface Bridge {
  state: BridgeState;
  /** One line for a card or a table cell. */
  text: string;
  tone: ChipTone;
}

export function bridgeOf(stack: Stack, status: StackStatus | undefined, forward: ForwardState | undefined, now: number): Bridge {
  if (!stack.forward_ports) return { state: "off", text: "bridge off", tone: "neutral" };
  if (forward?.up) {
    const ports = forward.ports.length === 1 ? "1 port" : `${forward.ports.length} ports`;
    const since = forward.since_ms ? ` · since ${clock(forward.since_ms)}${ago(now - forward.since_ms)}` : "";
    return { state: "connected", text: `bridge connected · ${ports}${since}`, tone: "good" };
  }
  if (forward && forward.ports.length > 0) {
    const attempt = forward.attempts > 0 ? `, attempt ${forward.attempts}` : "";
    const error = forward.error ? `: ${forward.error}` : "";
    return { state: "connecting", text: `bridge connecting${attempt}${error}`, tone: "warning" };
  }
  const running = status?.phase === "running" || status?.phase === "partial";
  return { state: "waiting", text: running ? "bridge starting" : "bridge waits for the stack to run", tone: "neutral" };
}

/** The line on a stack card: the state, and the switch. */
export function BridgeControl({ stack }: { stack: Stack }) {
  const status = useStore((state) => state.statuses[stack.id]);
  const forward = useStore((state) => state.forwards[stack.id]);
  const setStacks = useStore((state) => state.setStacks);
  const now = useNow(1000);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const bridge = bridgeOf(stack, status, forward, now);

  const flip = async () => {
    setWorking(true);
    setError(null);
    try {
      setStacks(await api.setForwardPorts(stack.id, !stack.forward_ports));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setWorking(false);
    }
  };

  return (
    <div className="flex items-center gap-2 text-[11px]">
      <span className={cx("h-1.5 w-1.5 shrink-0 rounded-full", DOT[bridge.state])} />
      <span className={cx("truncate", bridge.state === "connecting" ? "text-warning" : "text-ink-3")} title={bridge.text}>
        {bridge.text}
      </span>
      <Button size="sm" tone="ghost" onClick={() => void flip()} busy={working} title={stack.forward_ports ? "Drop the localhost ports for this stack" : "Hand this stack's ports to localhost again"}>
        {stack.forward_ports ? "Stop bridge" : "Start bridge"}
      </Button>
      {error ? <span className="text-critical">{error}</span> : null}
    </div>
  );
}

const DOT: Record<BridgeState, string> = { off: "bg-hairline", waiting: "border border-ink-3", connecting: "bg-warning pulse", connected: "bg-good pulse" };

export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** ` (3m)` once it has been up for a minute, so the time reads at a glance. */
function ago(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return "";
  if (minutes < 60) return ` (${minutes}m)`;
  const hours = Math.floor(minutes / 60);
  return hours < 24 ? ` (${hours}h ${minutes % 60}m)` : ` (${Math.floor(hours / 24)}d ${hours % 24}h)`;
}

export function useNow(everyMs: number): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), everyMs);
    return () => window.clearInterval(timer);
  }, [everyMs]);
  return now;
}
