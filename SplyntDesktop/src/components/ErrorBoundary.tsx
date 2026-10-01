import { Component, type ErrorInfo, type ReactNode } from "react";

type State = { message?: string };

/// A render error used to blank the window with no way back. The player is a
/// long-running app, so a crash offers a reload instead of a white screen.
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = {};

  static getDerivedStateFromError(error: unknown): State {
    return { message: error instanceof Error ? error.message : "Splynt hit an unexpected error." };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Splynt render error", error, info.componentStack);
  }

  render() {
    if (!this.state.message) return this.props.children;
    return (
      <main className="crash-page" role="alert">
        <h1>Splynt needs to restart</h1>
        <p>{this.state.message}</p>
        <p className="crash-page__note">Your library, downloads and saved logins are untouched.</p>
        <div>
          <button className="modal-primary" onClick={() => window.location.reload()} type="button">Reload Splynt</button>
          <button onClick={() => this.setState({ message: undefined })} type="button">Try to continue</button>
        </div>
      </main>
    );
  }
}
