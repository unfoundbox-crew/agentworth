import { Component, ErrorInfo, ReactNode } from 'react';

interface ErrorBoundaryProps {
  /** Shown in the fallback message, e.g. "Session inspector" — tells the
   *  user which panel broke without taking down the rest of the shell. */
  label: string;
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Catches a render error in one dashboard panel and shows an inline message
 * in its place instead of unmounting the whole app. Wrap each top-level
 * shell panel (session list, inspector, overview, coverage, archaeology,
 * exports) with one of these so a bug in one panel can't blank the rest of
 * the dashboard — see docs/DECISION-INBOX.md for the crash this replaced.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(`[ErrorBoundary:${this.props.label}]`, error, info.componentStack);
  }

  private reset = () => this.setState({ error: null });

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <div className="dashboard-error-boundary" role="alert">
        <p className="dashboard-error-boundary-title">{this.props.label} hit a render error</p>
        <p className="dashboard-error-boundary-detail">{error.message}</p>
        <button type="button" className="shell-retry-btn" onClick={this.reset}>
          Try again
        </button>
      </div>
    );
  }
}
