import { clock, plural, span } from "../lib/format";
import { api } from "../lib/ipc";
import type { ForwardState, Stack, StackStatus } from "../lib/types";
import { isUp, useStore } from "../state/store";
import type { DotState } from "../ui/Badges";
import { StatusDot } from "../ui/Badges";
import { Button, cx, ErrorLine } from "../ui/primitives";
import type { ChipTone } from "../ui/primitives";
import { useAction } from "../ui/useAction";
import { useNow } from "../ui/useNow";

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
    const since = forward.since_ms ? ` · since ${clock(forward.since_ms)}${upFor(now - forward.since_ms)}` : "";
    return { state: "connected", text: `bridge connected · ${plural(forward.ports.length, "port")}${since}`, tone: "good" };
  }
  if (forward && forward.ports.length > 0) {
    const attempt = forward.attempts > 0 ? `, attempt ${forward.attempts}` : "";
    const error = forward.error ? `: ${forward.error}` : "";
    return { state: "connecting", text: `bridge connecting${attempt}${error}`, tone: "warning" };
  }
  return { state: "waiting", text: isUp(status) ? "bridge starting" : "bridge waits for the stack to run", tone: "neutral" };
}

/** The line on a stack card: the state, and the switch. */
export function BridgeControl({ stack }: { stack: Stack }) {
  const status = useStore((state) => state.statuses[stack.id]);
  const forward = useStore((state) => state.forwards[stack.id]);
  const setStacks = useStore((state) => state.setStacks);
  // The clock only feeds "(3m)" on a connected bridge, which changes once a minute.
  const connected = stack.forward_ports && (forward?.up ?? false);
  const now = useNow(30_000, connected);
  const flip = useAction("inline");
  const bridge = bridgeOf(stack, status, forward, now);
  const dot = BRIDGE_DOT[bridge.state];

  return (
    <div>
      <div className="flex items-center gap-2 text-[11px]">
        <StatusDot state={dot.state} pulse={dot.pulse} label={bridge.text} size="sm" />
        <span className={cx("truncate", bridge.state === "connecting" ? "text-warning" : "text-ink-3")} title={bridge.text}>
          {bridge.text}
        </span>
        <Button
          size="sm"
          tone="ghost"
          onClick={() => void flip.run(async () => setStacks(await api.setForwardPorts(stack.id, !stack.forward_ports)))}
          busy={flip.busy}
          title={stack.forward_ports ? "Drop the localhost ports for this stack" : "Hand this stack's ports to localhost again"}
        >
          {stack.forward_ports ? "Stop bridge" : "Start bridge"}
        </Button>
      </div>
      <ErrorLine error={flip.error} className="mt-1" />
    </div>
  );
}

/** Live bridges pulse; one that is still connecting needs attention. */
const BRIDGE_DOT: Record<BridgeState, { state: DotState; pulse: boolean }> = {
  off: { state: "idle", pulse: false },
  waiting: { state: "pending", pulse: false },
  connecting: { state: "attention", pulse: true },
  connected: { state: "good", pulse: true },
};

/** ` (3m)` once it has been up for a minute, so the time reads at a glance. */
function upFor(ms: number): string {
  return ms < 60_000 ? "" : ` (${span(ms)})`;
}
