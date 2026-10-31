import { Component, type ReactNode } from "react";
import { EmptyState } from "./EmptyState.js";
import { NeonButton } from "./NeonButton.js";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  override state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  override componentDidCatch(error: Error) {
    // Visible in devtools; production reporting arrives with telemetry.
    console.error("[talos] render error:", error);
  }

  private handleReset = () => {
    this.setState({ error: null });
  };

  override render() {
    if (this.state.error !== null) {
      return (
        <div className="glass-panel p-5" role="alert">
          <EmptyState
            title="Something glitched in this panel"
            message={this.state.error.message}
            action={
              <NeonButton onClick={this.handleReset}>Reset panel</NeonButton>
            }
          />
        </div>
      );
    }
    return this.props.children;
  }
}
