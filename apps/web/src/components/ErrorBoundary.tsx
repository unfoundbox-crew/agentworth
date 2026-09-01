import React from 'react';

interface ErrorBoundaryProps {
  /** Short label for the panel this boundary wraps, shown in the fallback message. */
  label?: string;
  children: React.ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Catches render/lifecycle errors in the wrapped subtree so one broken panel
 * shows an inline message instead of unmounting the whole dashboard (React
 * error boundaries have no hook equivalent, hence the class component).
 */
export class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error(`ErrorBoundary caught an error in ${this.props.label || 'a panel'}:`, error, info.componentStack);
  }

  handleReset = () => {
    this.setState({ error: null });
  };

  render() {
    if (this.state.error) {
      return (
        <div className="border-2 border-red-600 dark:border-red-500 bg-red-50 dark:bg-red-950/30 p-5 font-mono text-xs text-red-800 dark:text-red-300">
          <div className="font-bold uppercase tracking-wide mb-1">
            {this.props.label || 'This panel'} failed to render
          </div>
          <div className="text-red-700/80 dark:text-red-400/80 mb-3">
            {this.state.error.message}
          </div>
          <button
            onClick={this.handleReset}
            className="px-3 py-1.5 bg-red-600 hover:bg-red-700 text-white font-bold border border-red-800 transition-colors"
          >
            Try again
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
