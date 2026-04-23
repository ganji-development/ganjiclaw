import { Component, type ReactNode, type ErrorInfo } from 'react';

interface ErrorBoundaryState {
  error: Error | null;
}

export class ErrorBoundary extends Component<
  { children: ReactNode },
  ErrorBoundaryState
> {
  constructor(props: { children: ReactNode }) {
    super(props);
    this.state = { error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('[ZeroClaw] Render error:', error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="p-6">
          <div className="card p-6 w-full max-w-lg" style={{ borderColor: 'rgba(239, 68, 68, 0.3)' }}>
            <h2 className="text-lg font-semibold mb-2" style={{ color: 'var(--color-status-error)' }}>
              Something went wrong
            </h2>
            <p className="text-sm mb-4" style={{ color: 'var(--pc-text-muted)' }}>
              A render error occurred. Check the browser console for details.
            </p>
            <pre className="text-xs rounded-lg p-3 overflow-x-auto whitespace-pre-wrap break-all font-mono" style={{ background: 'var(--pc-bg-base)', color: 'var(--color-status-error)' }}>
              {this.state.error.message}
            </pre>
            <button
              onClick={() => this.setState({ error: null })}
              className="btn-electric mt-6 px-4 py-2 text-sm font-medium"
            >
              Try again
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
