import { getCurrentWebview } from "@tauri-apps/api/webview";

import { isTauri } from "./ipc";

export interface DropHandlers {
  onHover: (hovering: boolean) => void;
  onDrop: (paths: string[]) => void;
}

/** What a browser drop pretends to be; the mock backend ignores the path. */
const MOCK_DROP = "/home/alex/projects/sample-stack";

/** Native file drops from Finder anywhere on the window. */
export function watchFileDrop(handlers: DropHandlers): () => void {
  if (isTauri) {
    const pending = getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === "enter" || payload.type === "over") handlers.onHover(true);
      else if (payload.type === "leave") handlers.onHover(false);
      else if (payload.type === "drop") {
        handlers.onHover(false);
        handlers.onDrop(payload.paths);
      }
    });
    return () => void pending.then((unlisten) => unlisten());
  }

  const over = (event: DragEvent) => {
    event.preventDefault();
    handlers.onHover(true);
  };
  const leave = () => handlers.onHover(false);
  const drop = (event: DragEvent) => {
    event.preventDefault();
    handlers.onHover(false);
    handlers.onDrop([MOCK_DROP]);
  };
  window.addEventListener("dragover", over);
  window.addEventListener("dragleave", leave);
  window.addEventListener("drop", drop);
  return () => {
    window.removeEventListener("dragover", over);
    window.removeEventListener("dragleave", leave);
    window.removeEventListener("drop", drop);
  };
}
