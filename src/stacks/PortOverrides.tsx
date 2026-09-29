import { useEffect, useState } from "react";

import { api } from "../lib/ipc";

/** The local port each published port gets on this computer, and which of
 * the chosen ones something here already listens on. Only a port the user
 * changed is in `overrides`; the rest keep their own number. The busy check
 * runs again whenever the chosen ports change. */
export function usePortOverrides(published: number[]) {
  const [overrides, setOverrides] = useState<Record<number, number>>({});
  const [busy, setBusy] = useState<number[]>([]);
  const localFor = (port: number) => overrides[port] ?? port;
  const localPorts = published.map(localFor);
  const localKey = localPorts.join(",");

  useEffect(() => {
    if (!localKey) return;
    let current = true;
    const ports = localKey.split(",").map(Number);
    api
      .busyPorts(ports)
      .then((taken) => current && setBusy(taken))
      .catch(console.warn);
    return () => {
      current = false;
    };
  }, [localKey]);

  const setLocal = (port: number, local: number) => {
    const next = { ...overrides };
    const own = local === port || !local;
    if (own) delete next[port];
    else next[port] = local;
    setOverrides(next);
  };

  const isTaken = (port: number) => busy.includes(localFor(port));
  const conflicts = published.filter(isTaken);
  // As the backend takes them: keyed by the published port, as text.
  const forRequest = Object.fromEntries(Object.entries(overrides).map(([port, local]) => [String(port), local]));

  return { localFor, setLocal, isTaken, conflicts, forRequest };
}

/** The small field for one port's number on this computer, with a one-click
 * way out when that number is already taken here. */
export function LocalPortField({
  port,
  local,
  taken,
  disabled = false,
  label,
  onChange,
}: {
  port: number;
  local: number;
  taken: boolean;
  disabled?: boolean;
  label: string;
  onChange: (local: number) => void;
}) {
  return (
    <>
      <input
        aria-label={label}
        className="tabular w-16 rounded-md border border-line bg-surface-2 px-1.5 py-0.5 text-[12px] text-ink outline-none focus:border-accent disabled:opacity-50"
        value={local}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value.replace(/\D/g, "").slice(0, 5)))}
      />
      {taken ? (
        <button type="button" className="whitespace-nowrap text-[11px] text-warning underline-offset-2 hover:underline" onClick={() => onChange(port + 1000)}>
          in use here, try {port + 1000}
        </button>
      ) : null}
    </>
  );
}
