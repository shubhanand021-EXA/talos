import { Component, type ReactNode } from "react";
import { EmptyState } from "../../design-system/EmptyState.js";

interface SceneErrorBoundaryProps {
  children: ReactNode;
  onReset?: () => void;
}

/**
 * Scene-local error boundary: a WebGL/postprocessing failure degrades to a
 * polished panel instead of taking down the dashboard.
 */
export class SceneErrorBoundary extends Component<
  SceneErrorBoundaryProps,
  { error: Error | null }
> {
  override state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  override componentDidCatch(error: Error) {
    console.error("[talos] scene error:", error);
  }

  private handleReset = () => {
    this.props.onReset?.();
    this.setState({ error: null });
  };

  override render() {
    if (this.state.error !== null) {
      return (
        <div
          role="alert"
          className="flex h-full items-center justify-center p-6"
        >
          <EmptyState
            title="Viewport recovered"
            message="The 3D renderer hit a fault and was contained. The rest of Talos is unaffected."
            action={
              <button
                type="button"
                onClick={this.handleReset}
                className="mt-2 rounded-control border border-cyan-300/40 bg-cyan-400/10 px-4 py-2 text-sm text-cyan-200 hover:bg-cyan-400/25"
              >
                Retry viewport
              </button>
            }
          />
        </div>
      );
    }
    return this.props.children;
  }
}
