import { Routes, Route, Navigate } from 'react-router-dom';
import { useState, useEffect, type SyntheticEvent } from 'react';
import { ThemeProvider } from './contexts/ThemeContext';
import Layout from './components/layout/Layout';
import Dashboard from './pages/Dashboard';
import AgentChat from './pages/AgentChat';
import Tools from './pages/Tools';
import Cron from './pages/Cron';
import Integrations from './pages/Integrations';
import Memory from './pages/Memory';
import Config from './pages/Config';
import Cost from './pages/Cost';
import Logs from './pages/Logs';
import Doctor from './pages/Doctor';
import Pairing from './pages/Pairing';
import Canvas from './pages/Canvas';
import { AuthProvider, useAuth } from './hooks/useAuth';
import { DraftContext, useDraftStore } from './hooks/useDraft';
import { setLocale, type Locale } from './lib/i18n';
import { loadLocale, saveLocale } from './contexts/ThemeContextUtils';
import { basePath } from './lib/basePath';
import { getAdminPairCode } from './lib/api';
import { LocaleContext } from './contexts/LocaleContext';
import { ErrorBoundary } from './components/ErrorBoundary';

// ---------------------------------------------------------------------------
// Pairing dialog component
// ---------------------------------------------------------------------------

function PairingDialog({ onPair }: { onPair: (code: string) => Promise<void> }) {
  const [code, setCode] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [displayCode, setDisplayCode] = useState<string | null>(null);
  const [codeLoading, setCodeLoading] = useState(true);

  // Fetch the current pairing code (public endpoint works in Docker too)
  useEffect(() => {
    let cancelled = false;
    getAdminPairCode()
      .then((data) => {
        if (!cancelled && data.pairing_code) {
          setDisplayCode(data.pairing_code);
          setCode(data.pairing_code); // auto-fill so user just clicks "Pair"
        }
      })
      .catch(() => {
        // Endpoint not reachable — user must check terminal / docker logs
      })
      .finally(() => {
        if (!cancelled) setCodeLoading(false);
      });
    return () => { cancelled = true; };
  }, []);

  const handleSubmit = async (e: SyntheticEvent) => {
    e.preventDefault();
    setLoading(true);
    setError('');
    try {
      await onPair(code);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Pairing failed');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center" style={{ background: 'var(--pc-bg-base)' }}>
      {/* Ambient glow */}
      <div className="relative surface-panel p-8 w-full max-w-md animate-fade-in-scale">

        <div className="text-center mb-8">
          <img
            src={`${basePath}/_app/zeroclaw-trans.png`}
            alt="ZeroClaw"
            className="h-20 w-20 rounded-2xl object-cover mx-auto mb-4 animate-float"
            onError={(e) => { e.currentTarget.style.display = 'none'; }}
          />
          <h1 className="text-2xl font-bold mb-2 text-gradient-blue">ZeroClaw</h1>
          <p className="text-sm" style={{ color: 'var(--pc-text-muted)' }}>
            {displayCode ? 'Your pairing code — click Pair to connect' : 'Enter the pairing code from your terminal'}
          </p>
        </div>

        {/* Show the pairing code if available (localhost) */}
        {!codeLoading && displayCode && (
          <div className="mb-6 p-4 rounded-2xl text-center border" style={{ background: 'var(--pc-accent-glow)', borderColor: 'var(--pc-accent-dim)' }}>
            <div className="text-4xl font-mono font-bold tracking-[0.4em] py-2" style={{ color: 'var(--pc-text-primary)' }}>
              {displayCode}
            </div>
            <p className="text-xs mt-2" style={{ color: 'var(--pc-text-muted)' }}>Enter this code below or on another device</p>
          </div>
        )}

        <form onSubmit={handleSubmit}>
          <input
            type="text"
            value={code}
            onChange={(e) => setCode(e.target.value)}
            placeholder="6-digit code"
            className="input-electric w-full px-4 py-4 text-center text-2xl tracking-[0.3em] font-medium mb-4"
            maxLength={6}
            autoFocus
          />
          {error && (
            <p aria-live="polite" className="text-sm mb-4 text-center animate-fade-in" style={{ color: 'var(--color-status-error)' }}>{error}</p>
          )}
          <button
            type="submit"
            disabled={loading || code.length < 6}
            className="btn-electric w-full py-3.5 text-sm font-semibold tracking-wide"
          >
            {loading ? (
              <span className="flex items-center justify-center gap-2">
                <span className="h-4 w-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                Pairing...
              </span>
            ) : 'Pair'}
          </button>
        </form>
      </div>
    </div>
  );
}

function AppContent() {
  const { isAuthenticated, requiresPairing, loading, pair, logout } = useAuth();
  const [locale, setLocaleState] = useState(loadLocale());
  const draftStore = useDraftStore();
  setLocale(locale as Locale);

  const setAppLocale = (newLocale: string) => {
    setLocaleState(newLocale);
    setLocale(newLocale as Locale);
    saveLocale(newLocale);
  };

  // Listen for 401 events to force logout
  useEffect(() => {
    const handler = () => {
      logout();
    };
    window.addEventListener('zeroclaw-unauthorized', handler);
    return () => window.removeEventListener('zeroclaw-unauthorized', handler);
  }, [logout]);

  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center" style={{ background: 'var(--pc-bg-base)' }}>
        <div className="flex flex-col items-center gap-4 animate-fade-in">
          <div className="h-10 w-10 border-2 rounded-full animate-spin" style={{ borderColor: 'var(--pc-border)', borderTopColor: 'var(--pc-accent)' }} />
          <p className="text-sm" style={{ color: 'var(--pc-text-muted)' }}>Connecting...</p>
        </div>
      </div>
    );
  }

  if (!isAuthenticated && requiresPairing) {
    return <PairingDialog onPair={pair} />;
  }

  return (
    <DraftContext.Provider value={draftStore}>
      <LocaleContext.Provider value={{ locale, setAppLocale }}>
        <Routes>
          <Route element={<Layout />}>
            <Route path="/" element={<Dashboard />} />
            <Route path="/agent" element={<AgentChat />} />
            <Route path="/tools" element={<Tools />} />
            <Route path="/cron" element={<Cron />} />
            <Route path="/integrations" element={<Integrations />} />
            <Route path="/memory" element={<Memory />} />
            <Route path="/config" element={<Config />} />
            <Route path="/cost" element={<Cost />} />
            <Route path="/logs" element={<Logs />} />
            <Route path="/doctor" element={<Doctor />} />
            <Route path="/pairing" element={<Pairing />} />
            <Route path="/canvas" element={<Canvas />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
      </LocaleContext.Provider>
    </DraftContext.Provider>
  );
}

export default function App() {
  return (
    <ErrorBoundary>
      <AuthProvider>
        <ThemeProvider>
          <AppContent />
        </ThemeProvider>
      </AuthProvider>
    </ErrorBoundary>
  );
}
