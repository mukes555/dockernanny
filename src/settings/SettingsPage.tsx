import { useEffect, useState } from "react";

import { errorMessage } from "../lib/ipc";
import type { Settings } from "../lib/types";
import { useStore } from "../state/store";
import type { SettingsSection } from "../state/store";
import { SpinnerIcon } from "../ui/icons";
import { Page } from "../ui/Page";
import { cx } from "../ui/primitives";
import { AdvancedSection, GeneralSection, MachinesSection, SharingSection } from "./SettingsSections";
import { UpdatesSection } from "./UpdatesSection";

/** Every choice is saved as soon as it is made; text fields save when the
 * user leaves them, so a half-typed path never lands in the file. One
 * section shows at a time, picked from the list on the left. */
export function SettingsPage() {
  const settings = useStore((state) => state.settings);
  const saveSettings = useStore((state) => state.saveSettings);
  const chosen = useStore((state) => state.settingsSection);
  const openSettings = useStore((state) => state.openSettings);
  const [draft, setDraft] = useState<Settings | null>(settings);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setDraft(settings), [settings]);

  if (!draft) {
    return (
      <Page title="Settings">
        <div className="flex items-center gap-2 text-[13px] text-ink-2">
          <SpinnerIcon /> Loading settings…
        </div>
      </Page>
    );
  }

  const commit = (change: Partial<Settings>) => {
    const next = { ...draft, ...change };
    setDraft(next);
    void saveSettings(next).catch((err) => setError(errorMessage(err)));
  };

  // A role's section is only listed while the role is on.
  const sections: Array<{ id: SettingsSection; label: string }> = [
    { id: "general", label: "General" },
    { id: "updates", label: "Updates" },
    ...(draft.use_machines ? [{ id: "machines" as const, label: "Machines and stacks" }] : []),
    ...(draft.share_this_computer ? [{ id: "sharing" as const, label: "Sharing" }] : []),
    { id: "advanced", label: "Advanced" },
  ];
  const section = sections.some((s) => s.id === chosen) ? chosen : "general";
  const props = { draft, setDraft, commit, onError: setError };

  return (
    <Page title="Settings" summary="Saved as you change them">
      <div className="grid grid-cols-[168px_minmax(0,1fr)] gap-8">
        <nav aria-label="Settings sections" className="sticky top-24 flex flex-col gap-0.5 self-start">
          {sections.map((item) => (
            <button
              key={item.id}
              type="button"
              onClick={() => openSettings(item.id)}
              aria-current={item.id === section ? "page" : undefined}
              className={cx(
                "rounded-lg px-3 py-1.5 text-left text-[13px] transition",
                item.id === section ? "bg-accent-soft font-medium text-ink" : "text-ink-2 hover:bg-surface-2 hover:text-ink",
              )}
            >
              {item.label}
            </button>
          ))}
        </nav>
        <div className="max-w-2xl min-w-0 space-y-4">
          {error ? <div className="text-[12px] text-critical">{error}</div> : null}
          {section === "general" ? <GeneralSection {...props} /> : null}
          {section === "updates" ? <UpdatesSection {...props} /> : null}
          {section === "machines" ? <MachinesSection {...props} /> : null}
          {section === "sharing" ? <SharingSection {...props} /> : null}
          {section === "advanced" ? <AdvancedSection {...props} /> : null}
        </div>
      </div>
    </Page>
  );
}
