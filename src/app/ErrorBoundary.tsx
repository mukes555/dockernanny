import { Component, type ErrorInfo, type ReactNode } from "react";

import { api, errorMessage } from "../lib/ipc";
import { LogoMark } from "../ui/icons";
import { Button } from "../ui/primitives";

interface State {
  error: Error | null;
  copied: string | null;
}

/** The last line of defence: a render error anywhere below here shows a
 * recoverable screen instead of a blank window. Async failures are handled
 * where they happen and surfaced as notices; this catches the rest. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null, copied: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  /** The redacted report plus the render error, ready for an issue. */
  async copyDiagnostics() {
    try {
      const report = await api.diagnostics();
      await api.copyText(`${report}\n--- window error ---\n${this.state.error?.message ?? ""}\n`);
      this.setState({ copied: "Copied, with names and addresses replaced. Paste it into an issue." });
    } catch (err) {
      this.setState({ copied: `Could not copy: ${errorMessage(err)}` });
    }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Kept for the console and any future crash log; the screen is the user's part.
    console.error("dockerNanny render error", error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="flex h-full items-center justify-center p-6">
        <div className="w-full max-w-md rounded-2xl border border-line bg-surface p-6 text-center">
          <LogoMark size={36} className="mx-auto text-accent" />
          <h1 className="mt-4 text-lg font-semibold tracking-tight text-ink">Something in the window broke</h1>
          <p className="mt-2 text-[13px] leading-relaxed text-ink-2">
            The background work (your stacks, forwards and the sharing role) keeps running. Reloading the window usually fixes the display.
          </p>
          <pre className="mono selectable mt-4 max-h-32 overflow-auto rounded-lg bg-plane/60 px-3 py-2 text-left text-[11px] text-ink-3">{error.message}</pre>
          <div className="mt-5 flex justify-center gap-2">
            <Button tone="primary" onClick={() => window.location.reload()}>
              Reload the window
            </Button>
            <Button onClick={() => this.setState({ error: null })}>Try again</Button>
            <Button onClick={() => void this.copyDiagnostics()}>Copy diagnostics</Button>
          </div>
          {this.state.copied ? <p className="mt-3 text-[12px] text-ink-3">{this.state.copied}</p> : null}
        </div>
      </div>
    );
  }
}
