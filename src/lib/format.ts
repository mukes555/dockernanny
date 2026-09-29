// How times, sizes and counts read everywhere in the app: one wording each.

/** `14:05`, in the user's own clock format. */
export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** A length of time the way a person says it: `3m`, `2h 5m`, `1d 4h`. */
export function span(ms: number): string {
  const minutes = Math.max(0, Math.floor(ms / 60_000));
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ${minutes % 60}m`;
  return `${Math.floor(hours / 24)}d ${hours % 24}h`;
}

/** How long ago: `12s ago`, `3m ago`, `2h ago`. */
export function ago(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.round(minutes / 60)}h ago`;
}

/** A running stopwatch: `45s`, `3m 07s`. */
export function stopwatch(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`;
}

/** Bytes as disks and transfers show them: `640 MB`, `1.2 GB`, `250 GB`. */
export function byteSize(bytes: number): string {
  if (bytes < 1e9) return `${Math.round(bytes / 1e6)} MB`;
  return `${(bytes / 1e9).toFixed(bytes >= 100e9 ? 0 : 1)} GB`;
}

/** Megabytes of memory as gigabytes with one decimal, without the unit: `15.6`. */
export function memoryGb(mb: number): string {
  return (mb / 1024).toFixed(1);
}

/** `1 stack`, `3 stacks`; words that do not just add an s pass their plural. */
export function plural(count: number, one: string, many = `${one}s`): string {
  return `${count} ${count === 1 ? one : many}`;
}
