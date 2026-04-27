import { useCallback, useEffect, useState } from 'react';
import {
  Sparkles,
  Search,
  Trash2,
  Download,
  Power,
  PowerOff,
  RefreshCw,
  ChevronDown,
  ChevronRight,
  AlertCircle,
  CheckCircle,
} from 'lucide-react';
import type { InstalledSkill, ClawhubSearchResult } from '@/types/api';
import {
  getSkills,
  installSkill,
  uninstallSkill,
  enableSkill,
  disableSkill,
  searchClawhub,
} from '@/lib/api';

type ToastKind = 'ok' | 'error';
interface Toast {
  kind: ToastKind;
  message: string;
}

export default function Skills() {
  const [installed, setInstalled] = useState<InstalledSkill[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);
  const [installedError, setInstalledError] = useState<string | null>(null);
  const [filter, setFilter] = useState('');
  const [expanded, setExpanded] = useState<string | null>(null);
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);

  const [installedSectionOpen, setInstalledSectionOpen] = useState(true);
  const [browseSectionOpen, setBrowseSectionOpen] = useState(true);

  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<ClawhubSearchResult[]>([]);
  const [searchLoading, setSearchLoading] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [searchAttempted, setSearchAttempted] = useState(false);
  const [installingSlug, setInstallingSlug] = useState<string | null>(null);

  const [customSource, setCustomSource] = useState('');
  const [customInstalling, setCustomInstalling] = useState(false);

  const fetchInstalled = useCallback(() => {
    setInstalledLoading(true);
    setInstalledError(null);
    getSkills()
      .then(setInstalled)
      .catch((err: Error) => setInstalledError(err.message))
      .finally(() => setInstalledLoading(false));
  }, []);

  useEffect(() => {
    fetchInstalled();
  }, [fetchInstalled]);

  // Auto-dismiss toast after 4s
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(t);
  }, [toast]);

  const showToast = (kind: ToastKind, message: string) => setToast({ kind, message });

  const filtered = installed.filter((s) => {
    if (!filter.trim()) return true;
    const q = filter.toLowerCase();
    return (
      s.name.toLowerCase().includes(q) ||
      s.description.toLowerCase().includes(q) ||
      (s.id ?? '').toLowerCase().includes(q) ||
      s.tags.some((tag) => tag.toLowerCase().includes(q))
    );
  });

  const onToggleEnabled = async (skill: InstalledSkill) => {
    if (!skill.id) return;
    setPendingId(skill.id);
    try {
      if (skill.enabled) await disableSkill(skill.id);
      else await enableSkill(skill.id);
      showToast('ok', `${skill.name} ${skill.enabled ? 'disabled' : 'enabled'}`);
      fetchInstalled();
    } catch (err) {
      showToast('error', err instanceof Error ? err.message : 'Toggle failed');
    } finally {
      setPendingId(null);
    }
  };

  const onUninstall = async (skill: InstalledSkill) => {
    if (!skill.id) return;
    if (!window.confirm(`Uninstall ${skill.name}? This deletes the skill folder.`)) return;
    setPendingId(skill.id);
    try {
      await uninstallSkill(skill.id);
      showToast('ok', `${skill.name} uninstalled`);
      fetchInstalled();
    } catch (err) {
      showToast('error', err instanceof Error ? err.message : 'Uninstall failed');
    } finally {
      setPendingId(null);
    }
  };

  const runSearch = async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    const q = searchQuery.trim();
    if (!q) {
      setSearchResults([]);
      setSearchAttempted(false);
      return;
    }
    setSearchLoading(true);
    setSearchError(null);
    setSearchAttempted(true);
    try {
      const resp = await searchClawhub(q);
      setSearchResults(resp.results ?? []);
    } catch (err) {
      setSearchError(err instanceof Error ? err.message : 'Search failed');
      setSearchResults([]);
    } finally {
      setSearchLoading(false);
    }
  };

  const onInstallSlug = async (slug: string) => {
    setInstallingSlug(slug);
    try {
      const result = await installSkill(`clawhub:${slug}`);
      showToast('ok', `Installed to ${result.installed_path}`);
      fetchInstalled();
    } catch (err) {
      showToast('error', err instanceof Error ? err.message : 'Install failed');
    } finally {
      setInstallingSlug(null);
    }
  };

  const onInstallCustom = async (e: React.FormEvent) => {
    e.preventDefault();
    const source = customSource.trim();
    if (!source) return;
    setCustomInstalling(true);
    try {
      const result = await installSkill(source);
      showToast('ok', `Installed to ${result.installed_path}`);
      setCustomSource('');
      fetchInstalled();
    } catch (err) {
      showToast('error', err instanceof Error ? err.message : 'Install failed');
    } finally {
      setCustomInstalling(false);
    }
  };

  return (
    <div className="p-6 space-y-6 animate-fade-in">
      {/* Toast */}
      {toast && (
        <div
          aria-live="polite"
          className="fixed top-4 right-4 z-50 flex items-center gap-2 px-4 py-3 rounded-xl border animate-fade-in"
          style={{
            background:
              toast.kind === 'ok'
                ? 'rgba(34, 197, 94, 0.10)'
                : 'rgba(239, 68, 68, 0.10)',
            borderColor:
              toast.kind === 'ok'
                ? 'rgba(34, 197, 94, 0.35)'
                : 'rgba(239, 68, 68, 0.35)',
            color: toast.kind === 'ok' ? '#4ade80' : '#f87171',
          }}
        >
          {toast.kind === 'ok' ? (
            <CheckCircle className="h-4 w-4 flex-shrink-0" />
          ) : (
            <AlertCircle className="h-4 w-4 flex-shrink-0" />
          )}
          <span className="text-sm">{toast.message}</span>
        </div>
      )}

      {/* ── Installed section ───────────────────────────────────── */}
      <div>
        <button
          onClick={() => setInstalledSectionOpen((v) => !v)}
          className="flex items-center gap-2 mb-4 w-full text-left group"
          style={{ background: 'transparent', border: 'none', cursor: 'pointer', padding: 0 }}
          aria-expanded={installedSectionOpen}
          aria-controls="installed-skills-section"
        >
          <Sparkles className="h-5 w-5" style={{ color: 'var(--pc-accent)' }} />
          <span
            className="text-sm font-semibold uppercase tracking-wider flex-1"
            role="heading"
            aria-level={2}
            style={{ color: 'var(--pc-text-primary)' }}
          >
            Installed Skills ({installed.length})
          </span>
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              fetchInstalled();
            }}
            className="p-1.5 rounded-md mr-1 transition-all"
            style={{ background: 'transparent', color: 'var(--pc-text-muted)' }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = 'var(--pc-hover)';
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = 'transparent';
            }}
            title="Refresh"
            aria-label="Refresh installed skills"
          >
            <RefreshCw className={`h-4 w-4 ${installedLoading ? 'animate-spin' : ''}`} />
          </button>
          <ChevronDown
            className="h-4 w-4 opacity-40 group-hover:opacity-100"
            style={{
              color: 'var(--pc-text-muted)',
              transform: installedSectionOpen ? 'rotate(0deg)' : 'rotate(-90deg)',
              transition: 'transform 0.2s ease, opacity 0.2s ease',
            }}
          />
        </button>

        <div id="installed-skills-section">
          {installedSectionOpen && (
            <>
              {/* Filter */}
              <div className="relative max-w-md mb-4">
                <Search
                  className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4"
                  style={{ color: 'var(--pc-text-faint)' }}
                />
                <input
                  type="text"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  placeholder="Filter by name, description, tag..."
                  className="input-electric w-full pl-10 pr-4 py-2.5 text-sm"
                />
              </div>

              {installedError ? (
                <div
                  className="rounded-2xl border p-4"
                  style={{
                    background: 'rgba(239, 68, 68, 0.08)',
                    borderColor: 'rgba(239, 68, 68, 0.2)',
                    color: '#f87171',
                  }}
                >
                  Failed to load skills: {installedError}
                </div>
              ) : installedLoading ? (
                <div className="flex items-center justify-center h-32">
                  <div
                    className="h-8 w-8 border-2 rounded-full animate-spin"
                    style={{ borderColor: 'var(--pc-border)', borderTopColor: 'var(--pc-accent)' }}
                  />
                </div>
              ) : filtered.length === 0 ? (
                <p className="text-sm" style={{ color: 'var(--pc-text-muted)' }}>
                  {installed.length === 0
                    ? 'No skills installed yet. Search for a skill below to install one.'
                    : 'No skills match your filter.'}
                </p>
              ) : (
                <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4 stagger-children">
                  {filtered.map((skill) => {
                    const isExpanded = expanded === skill.id;
                    const busy = pendingId === skill.id;
                    return (
                      <div
                        key={skill.id ?? `${skill.name}:${skill.location ?? ''}`}
                        className="card overflow-hidden animate-slide-in-up"
                        style={{
                          opacity: skill.enabled ? 1 : 0.65,
                        }}
                      >
                        <button
                          onClick={() => setExpanded(isExpanded ? null : skill.id)}
                          className="w-full text-left p-4 transition-all"
                          style={{ background: 'transparent' }}
                          onMouseEnter={(e) => {
                            e.currentTarget.style.background = 'var(--pc-hover)';
                          }}
                          onMouseLeave={(e) => {
                            e.currentTarget.style.background = 'transparent';
                          }}
                        >
                          <div className="flex items-start justify-between gap-2">
                            <div className="flex items-center gap-2 min-w-0">
                              <Sparkles
                                className="h-4 w-4 flex-shrink-0"
                                style={{ color: 'var(--pc-accent)' }}
                              />
                              <h3
                                className="text-sm font-semibold truncate"
                                style={{ color: 'var(--pc-text-primary)' }}
                                title={skill.name}
                              >
                                {skill.name}
                              </h3>
                              <span
                                className="text-[10px] px-1.5 py-0.5 rounded font-mono"
                                style={{
                                  background: 'var(--pc-bg-base)',
                                  color: 'var(--pc-text-muted)',
                                }}
                              >
                                v{skill.version}
                              </span>
                            </div>
                            <div className="flex items-center gap-2">
                              <span
                                className="inline-flex items-center px-2 py-0.5 rounded-full text-[10px] font-semibold border"
                                style={{
                                  background: skill.enabled
                                    ? 'rgba(34, 197, 94, 0.10)'
                                    : 'rgba(148, 163, 184, 0.10)',
                                  borderColor: skill.enabled
                                    ? 'rgba(34, 197, 94, 0.30)'
                                    : 'var(--pc-border)',
                                  color: skill.enabled ? '#4ade80' : 'var(--pc-text-muted)',
                                }}
                              >
                                {skill.enabled ? 'enabled' : 'disabled'}
                              </span>
                              {isExpanded ? (
                                <ChevronDown
                                  className="h-4 w-4"
                                  style={{ color: 'var(--pc-accent)' }}
                                />
                              ) : (
                                <ChevronRight
                                  className="h-4 w-4"
                                  style={{ color: 'var(--pc-text-faint)' }}
                                />
                              )}
                            </div>
                          </div>
                          <p
                            className="text-sm mt-2 line-clamp-2"
                            style={{ color: 'var(--pc-text-muted)' }}
                          >
                            {skill.description || 'No description.'}
                          </p>
                          {skill.tags.length > 0 && (
                            <div className="flex flex-wrap gap-1 mt-2">
                              {skill.tags.slice(0, 4).map((tag) => (
                                <span
                                  key={tag}
                                  className="text-[10px] px-1.5 py-0.5 rounded"
                                  style={{
                                    background: 'var(--pc-accent-glow)',
                                    color: 'var(--pc-text-secondary)',
                                  }}
                                >
                                  {tag}
                                </span>
                              ))}
                            </div>
                          )}
                        </button>

                        {isExpanded && (
                          <div
                            className="border-t p-4 animate-fade-in space-y-3"
                            style={{ borderColor: 'var(--pc-border)' }}
                          >
                            {skill.author && (
                              <div className="text-xs" style={{ color: 'var(--pc-text-muted)' }}>
                                <span className="font-semibold">Author:</span> {skill.author}
                              </div>
                            )}
                            {skill.id && (
                              <div className="text-xs font-mono" style={{ color: 'var(--pc-text-muted)' }}>
                                <span className="font-semibold font-sans">ID:</span> {skill.id}
                              </div>
                            )}
                            {skill.location && (
                              <div
                                className="text-xs font-mono break-all"
                                style={{ color: 'var(--pc-text-faint)' }}
                              >
                                {skill.location}
                              </div>
                            )}
                            <div className="text-xs" style={{ color: 'var(--pc-text-muted)' }}>
                              {skill.tool_count} tool{skill.tool_count === 1 ? '' : 's'} ·{' '}
                              {skill.prompt_count} prompt{skill.prompt_count === 1 ? '' : 's'}
                            </div>
                            <div className="flex flex-wrap gap-2 pt-2">
                              <button
                                disabled={busy || !skill.id}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  onToggleEnabled(skill);
                                }}
                                className="btn-electric inline-flex items-center gap-1.5 text-xs px-3 py-1.5"
                              >
                                {skill.enabled ? (
                                  <>
                                    <PowerOff className="h-3.5 w-3.5" /> Disable
                                  </>
                                ) : (
                                  <>
                                    <Power className="h-3.5 w-3.5" /> Enable
                                  </>
                                )}
                              </button>
                              <button
                                disabled={busy || !skill.id}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  onUninstall(skill);
                                }}
                                className="inline-flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-xl border transition-all"
                                style={{
                                  background: 'rgba(239, 68, 68, 0.08)',
                                  borderColor: 'rgba(239, 68, 68, 0.30)',
                                  color: '#f87171',
                                }}
                              >
                                <Trash2 className="h-3.5 w-3.5" /> Uninstall
                              </button>
                            </div>
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </>
          )}
        </div>
      </div>

      {/* ── Browse / Install section ──────────────────────────── */}
      <div className="animate-slide-in-up" style={{ animationDelay: '120ms' }}>
        <button
          onClick={() => setBrowseSectionOpen((v) => !v)}
          className="flex items-center gap-2 mb-4 w-full text-left group"
          style={{ background: 'transparent', border: 'none', cursor: 'pointer', padding: 0 }}
          aria-expanded={browseSectionOpen}
          aria-controls="browse-skills-section"
        >
          <Download className="h-5 w-5" style={{ color: 'var(--color-status-success)' }} />
          <span
            className="text-sm font-semibold uppercase tracking-wider flex-1"
            role="heading"
            aria-level={2}
            style={{ color: 'var(--pc-text-primary)' }}
          >
            Browse ClawHub
          </span>
          <ChevronDown
            className="h-4 w-4 opacity-40 group-hover:opacity-100"
            style={{
              color: 'var(--pc-text-muted)',
              transform: browseSectionOpen ? 'rotate(0deg)' : 'rotate(-90deg)',
              transition: 'transform 0.2s ease, opacity 0.2s ease',
            }}
          />
        </button>

        <div id="browse-skills-section">
          {browseSectionOpen && (
            <div className="space-y-4">
              <form onSubmit={runSearch} className="flex gap-2 max-w-2xl">
                <div className="relative flex-1">
                  <Search
                    className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4"
                    style={{ color: 'var(--pc-text-faint)' }}
                  />
                  <input
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder="Search clawhub.ai for skills..."
                    className="input-electric w-full pl-10 pr-4 py-2.5 text-sm"
                  />
                </div>
                <button
                  type="submit"
                  disabled={searchLoading || !searchQuery.trim()}
                  className="btn-electric px-4 py-2 text-sm font-semibold"
                >
                  {searchLoading ? 'Searching...' : 'Search'}
                </button>
              </form>

              {searchError && (
                <div
                  className="rounded-2xl border p-3 text-sm"
                  style={{
                    background: 'rgba(239, 68, 68, 0.08)',
                    borderColor: 'rgba(239, 68, 68, 0.2)',
                    color: '#f87171',
                  }}
                >
                  {searchError}
                </div>
              )}

              {searchAttempted && !searchLoading && !searchError && searchResults.length === 0 && (
                <p className="text-sm" style={{ color: 'var(--pc-text-muted)' }}>
                  No results.
                </p>
              )}

              {searchResults.length > 0 && (
                <div className="card overflow-hidden rounded-2xl">
                  <table className="table-electric">
                    <thead>
                      <tr>
                        <th>Name</th>
                        <th>Slug</th>
                        <th>Version</th>
                        <th></th>
                      </tr>
                    </thead>
                    <tbody>
                      {searchResults.map((r) => {
                        const slug = r.slug ?? '';
                        const isInstalling = installingSlug === slug;
                        const alreadyInstalled = installed.some(
                          (s) => s.id && slug && s.id === slug.replace(/-/g, '_'),
                        );
                        return (
                          <tr key={slug || `${r.displayName}-${r.score}`}>
                            <td>
                              <div
                                className="font-medium text-sm"
                                style={{ color: 'var(--pc-text-primary)' }}
                              >
                                {r.displayName ?? slug}
                              </div>
                              {r.summary && (
                                <div
                                  className="text-xs mt-1 line-clamp-2"
                                  style={{ color: 'var(--pc-text-muted)' }}
                                >
                                  {r.summary}
                                </div>
                              )}
                            </td>
                            <td className="font-mono text-xs" style={{ color: 'var(--pc-text-muted)' }}>
                              {slug || '-'}
                            </td>
                            <td style={{ color: 'var(--pc-text-muted)' }}>{r.version ?? '-'}</td>
                            <td>
                              <button
                                disabled={!slug || isInstalling || alreadyInstalled}
                                onClick={() => onInstallSlug(slug)}
                                className="btn-electric inline-flex items-center gap-1.5 text-xs px-3 py-1.5"
                              >
                                <Download className="h-3.5 w-3.5" />
                                {alreadyInstalled
                                  ? 'Installed'
                                  : isInstalling
                                  ? 'Installing...'
                                  : 'Install'}
                              </button>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              )}

              {/* Install from arbitrary source */}
              <div className="mt-6">
                <h3
                  className="text-xs font-semibold uppercase tracking-wider mb-2"
                  style={{ color: 'var(--pc-text-muted)' }}
                >
                  Install from URL or local path
                </h3>
                <p className="text-xs mb-3" style={{ color: 'var(--pc-text-faint)' }}>
                  Examples: <code>clawhub:my-slug</code>, <code>https://github.com/owner/skill.git</code>,
                  <code>/path/to/local/skill</code>
                </p>
                <form onSubmit={onInstallCustom} className="flex gap-2 max-w-2xl">
                  <input
                    type="text"
                    value={customSource}
                    onChange={(e) => setCustomSource(e.target.value)}
                    placeholder="clawhub:slug | git URL | local path"
                    className="input-electric flex-1 px-4 py-2.5 text-sm"
                  />
                  <button
                    type="submit"
                    disabled={customInstalling || !customSource.trim()}
                    className="btn-electric px-4 py-2 text-sm font-semibold"
                  >
                    {customInstalling ? 'Installing...' : 'Install'}
                  </button>
                </form>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
