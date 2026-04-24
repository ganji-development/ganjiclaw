# Activity Archive — Remaining Work

Forward-looking to-do list, stripped of completed items. For full context and done-items, see the [full checklist](./zeroclaw-activity-archive-checklist.md).

## Deferred from earlier phases

- [ ] **E2E sanity run.** Set `activity_archive.enabled = true` in [e:/zeroclaw/.zeroclaw/config.toml](../../e:/zeroclaw/.zeroclaw/config.toml), start `zeroclaw daemon`, tab between a few windows for ~30s, stop, then `sqlite3 activity_archive.db "SELECT source, value FROM raw_events LIMIT 20"`. Confirms the daemon-supervised collector actually writes events.
- [ ] **Migrate 7 shelved test files inline.** The `tests/*.rs.pending_migration` files access private APIs — they were unit tests misplaced into the external `tests/` dir. Move each one's content into `#[cfg(test)] mod tests { ... }` at the bottom of the corresponding `src/*.rs`. Fix API drift along the way: missing `Default` impls on `*Config` types in `runtime.rs`, `PathBuf`-doesn't-implement-`Display` in `collector_tests`, missing `Normalizer::redact`.
- [ ] **Make `IDLE_THRESHOLD_SECONDS` configurable.** Add `idle_threshold_seconds: u64` to `CollectorConfig` in [schema.rs](../../crates/zeroclaw-config/src/schema.rs), pass through the runtime config translation in [daemon/activity_archive.rs](../../crates/zeroclaw-runtime/src/daemon/activity_archive.rs), thread to `WindowFocusCollector::new`.
- [ ] **Mock-backed event stream test.** Feed synthetic `get_foreground_window_info` / `get_idle_seconds` into `WindowFocusCollector::start`'s stream loop, assert the sequence of emitted `RawEvent`s for window changes and idle transitions.

## Phase 2 — Implement the stub collectors

All four currently return `futures::stream::empty()`. In ascending difficulty:

### `file_activity`

- [ ] Build a `notify::RecommendedWatcher` over `config.collectors.file_activity_folders`.
- [ ] Debounce events (notify fires 3–4 `Modify` events per save).
- [ ] Map `Create / Modify / Remove / Rename` → `RawEvent { path, kind, timestamp }`.
- [ ] Apply `privacy.exclude_paths` at emit time (pre-DB).
- [ ] Test: write into a tempdir, assert the stream.

### `shell_activity`

- [ ] Tail `$env:APPDATA\Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt`. Store byte offsets in `ingestion_offsets` so restarts don't re-ingest.
- [ ] Check cmd.exe doskey history if present.
- [ ] Default to hashing commands; opt-in flag for raw capture (shell commands routinely contain secrets, paths, API keys).
- [ ] Test: write to a fake history file, assert events.

### `browser_history`

- [ ] Chrome: read `%LocalAppData%\Google\Chrome\User Data\Default\History` (SQLite). **Copy to tempfile before opening** — live file is locked while Chrome runs.
- [ ] Edge: same pattern at `%LocalAppData%\Microsoft\Edge\User Data\Default\History`.
- [ ] Firefox: `places.sqlite` under `%AppData%\Mozilla\Firefox\Profiles\*.default-release\`.
- [ ] Track last-seen `visit_time` per browser in `ingestion_offsets`.
- [ ] Apply `privacy.exclude_domains` before storing.
- [ ] Test: synthetic SQLite with sample rows.

### `process_launch`

- [ ] Choose ingestion: Security Event Log 4688 (needs audit-policy + admin) or ETW `Microsoft-Windows-Kernel-Process` provider. ETW is cleaner.
- [ ] Add `windows` crate features as needed (likely `Win32_System_EventLog` + whatever subscription APIs).
- [ ] **Document the privilege requirement.** This collector probably requires an elevated daemon; fail loudly if non-elevated rather than silently emitting nothing.

## Phase 3 — Verify the middle pipeline

The middle (`normalizer` / `sessionizer` / `summarizer`) has no TODOs but has never been end-to-end exercised in this fork.

- [ ] **Normalizer regression test.** `raw_events` → `events` on a synthetic window-focus stream. One positive case, one privacy-filtered case.
- [ ] **Sessionizer test.** Exercise `update_sessions()` (called every 60s from [runtime.rs:251](../../crates/zeroclaw-activity-archive/src/runtime.rs#L251)) with synthetic event streams; verify idle-timeout and context-switch-threshold thresholds from config.
- [ ] **Summarizer test.** Run `generate_hourly_summary` + `generate_daily_log` against a populated DB, sanity-check output format.

## Phase 4 — Agent tools

All tools live in `zeroclaw-tools` under `#[cfg(target_os = "windows")]`. Each opens a fresh read-only rusqlite connection per call — no shared state with the daemon.

### Read-side

- [ ] `activity_archive_view` — `{hours?, from?, to?, limit?}` → list of events.
- [ ] `activity_archive_list_sessions` — `{from, to}` → inferred sessions with duration and dominant app.
- [ ] `activity_archive_summarize` — `{date | from/to, granularity}` → summary text from `summaries` table.
- [ ] `activity_archive_search` — `{query, hours?}` → text search over titles / commands / URLs.
- [ ] `activity_archive_stats` — `{hours}` → aggregate: top apps, top domains, session count, total active time.

### Privacy-rule tools

- [ ] `activity_archive_privacy_list` — list rows from `privacy_rules`.
- [ ] `activity_archive_privacy_add` — `{rule_type: "path" | "title" | "domain", pattern}`.
- [ ] `activity_archive_privacy_remove` — `{id}`.

### Write-side

- [ ] `activity_archive_sync_notion` — enqueue in `notion_sync_queue`; daemon's sync loop picks it up. Return immediately, don't block the agent turn.

### Wiring

- [ ] Identify the `zeroclaw-tools` factory/registration pattern (not yet verified — do a pass before writing the first tool).
- [ ] Gate the whole bundle on `#[cfg(target_os = "windows")]`.

## Phase 5 — Finish Notion sync

- [ ] Implement the three `// TODO: Implement Notion API call` sites at [notion_sync.rs:190, 197, 204](../../crates/zeroclaw-activity-archive/src/notion_sync.rs) with `reqwest` POST to `api.notion.com/v1/pages`.
- [ ] Respect Notion rate limits (3 req/s per integration).
- [ ] Retry with exponential backoff on 429 / 5xx.
- [ ] Idempotency: use `notion_sync_queue` rows as the work list; mark `synced_at` on success.
- [ ] Test against a sandbox database or `wiremock` record+replay.

## Phase 6 — Polish & docs

- [ ] Config reference doc: every flag under `[activity_archive]`, what each collector gathers, privacy implications, what needs elevation.
- [ ] Sane default privacy rules on first run (password managers, incognito browsers, `.env` files, SSH key paths).
- [ ] Onboarding wizard touchpoint: explicit opt-in prompt with a plain-English list of what gets collected. Default = off.
- [ ] End-to-end integration test: daemon start → synthetic events → privacy filter → DB → tool query → expected result.

---

## Recommended order for Phase 4 specifically

1. `activity_archive_view` + `activity_archive_stats` — against window-focus data. Small, immediately useful.
2. `activity_archive_summarize` — depends on Phase 3 summarizer verification.
3. Privacy-rule tools — zero dependencies.
4. `activity_archive_search` + `activity_archive_list_sessions` — after data has accumulated.
5. `activity_archive_sync_notion` — after Phase 5.
