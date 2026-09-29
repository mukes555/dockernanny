import type { ReactNode } from "react";

import type { DoctorRow } from "../lib/types";
import { CheckIcon, SpinnerIcon, XIcon } from "../ui/icons";
import { CodeBlock, Inset } from "../ui/primitives";

/** The five things a machine needs, in the order the doctor checks them. */
const CHECKS: Array<{ key: string; label: string }> = [
  { key: "ssh", label: "SSH" },
  { key: "docker", label: "Docker" },
  { key: "compose", label: "Compose" },
  { key: "rsync", label: "rsync" },
  { key: "host", label: "Host" },
];

/** The doctor's results as they arrive: a spinner while a check runs, the
 * fix under a failed one, "skipped" for the rest once ssh failed. The same
 * rows show this computer's readiness, whose fixes are sentences, not commands. */
export function DoctorRows({
  rows,
  checking,
  checks = CHECKS,
  fixesAreCommands = true,
}: {
  rows: DoctorRow[];
  checking: boolean;
  checks?: Array<{ key: string; label: string }>;
  fixesAreCommands?: boolean;
}) {
  // A machine that does not answer ssh cannot be checked further; this computer has no such gate.
  const sshOk = rows.some((row) => row.key === "ssh" && row.ok);
  const skipped = fixesAreCommands && !checking && rows.length > 0 && !sshOk;
  return (
    <div className="space-y-1.5">
      {checks.map((check) => (
        <CheckRow
          key={check.key}
          label={check.label}
          row={rows.find((row) => row.key === check.key)}
          pending={checking}
          skipped={skipped}
          fixIsCommand={fixesAreCommands}
        />
      ))}
    </div>
  );
}

function CheckRow({
  label,
  row,
  pending,
  skipped,
  fixIsCommand,
}: {
  label: string;
  row?: DoctorRow;
  pending: boolean;
  skipped: boolean;
  fixIsCommand: boolean;
}) {
  const detail = row ? row.detail : checkWord(skipped, pending);
  return (
    <Inset>
      <div className="flex items-center gap-2.5 text-[12px]">
        <span className="flex w-4 justify-center">
          <CheckMark row={row} pending={pending} />
        </span>
        <span className="w-24 shrink-0 font-medium text-ink">{label}</span>
        <span className="min-w-0 flex-1 truncate text-ink-2" title={detail}>
          {detail}
        </span>
      </div>
      {row?.fix ? (
        <div className="mt-2 ml-6">{fixIsCommand ? <CodeBlock code={row.fix} /> : <p className="text-[12px] leading-relaxed text-ink-2">{row.fix}</p>}</div>
      ) : null}
    </Inset>
  );
}

/** A tick or a cross once the check answered, a spinner while it runs; the
 * outcome is also written out for screen readers, which skip the icons. */
function CheckMark({ row, pending }: { row?: DoctorRow; pending: boolean }) {
  if (row?.ok) return <Marked icon={<CheckIcon className="text-good" />} said="passed" />;
  if (row) return <Marked icon={<XIcon className="text-critical" />} said="failed" />;
  if (pending) return <SpinnerIcon className="text-ink-3" />;
  return <span className="inline-block h-3.5 w-3.5" />;
}

function Marked({ icon, said }: { icon: ReactNode; said: string }) {
  return (
    <>
      {icon}
      <span className="sr-only">{said}</span>
    </>
  );
}

/** What a check that has not answered says. */
function checkWord(skipped: boolean, pending: boolean): string {
  if (skipped) return "skipped";
  if (pending) return "checking";
  return "";
}
