import { Component, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";
import { applyTheme, initialTheme } from "./lib/theme";

applyTheme(initialTheme());

/** Last resort: show what broke and offer a way out instead of a blank window. */
class Crash extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (!this.state.error) return this.props.children;
    const clear = () => {
      try {
        for (const k of ["lifeline.plan.v2", "lifeline.airdrop.v1"]) localStorage.removeItem(k);
      } catch {
        /* storage unavailable */
      }
      location.reload();
    };
    return (
      <div className="empty">
        <div>
          <h1 className="empty-title">Something went wrong</h1>
          <p>{this.state.error.message}</p>
          <p className="empty-actions">
            <button className="btn" onClick={() => location.reload()}>
              Reload
            </button>
            <button className="btn" onClick={clear}>
              Reset scenarios and reload
            </button>
          </p>
        </div>
      </div>
    );
  }
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Crash>
      <App />
    </Crash>
  </StrictMode>,
);
