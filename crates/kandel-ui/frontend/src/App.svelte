<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import {
    deleteAppConfig,
    getFullConfig,
    getCommandExtensions,
    getInitialStatuses,
    onStateUpdate,
    selectCommandPath,
    updateAppConfig,
  } from "./lib/api";
  import {
    applyStateUpdate,
    clampInt,
    extractAppNameFromPath,
    formatClock,
    formatCommandLine,
    isSupportedCommandFile,
    logRows,
    mergeLedgerRows,
    normalizeEnv,
    sanitizeEnvKey,
    stateView,
    summarize,
    type LogRow,
  } from "./lib/format";
  import { onLogUpdate, getAppLogs } from "./lib/api";
  import { makeEmptyForm, type AppConfig, type AppStateInfo, type LogEvent } from "./lib/types";
  import ResizeHandles from "./lib/ResizeHandles.svelte";
  import Titlebar from "./lib/Titlebar.svelte";
  import { isWindowMaximized, onWindowResized } from "./lib/window";
  import {
    applyTheme,
    nextSetting,
    readSetting,
    resolveTheme,
    syncWindowTheme,
    writeSetting,
    type ThemeSetting,
  } from "./lib/theme";

  // ── Ledger state ────────────────────────────────────────────────
  // Two sources, kept apart:
  //   configEntries — every entry in config.toml (the source of truth)
  //   liveApps      — statuses pushed by the core over IPC
  // The ledger merges them per field: config decides *which* rows exist and
  // *what* they are (command, args, enabled), so a saved edit shows at once;
  // live status supplies *how* they run (state, pid, restart count).
  let configEntries = $state<Record<string, AppConfig>>({});
  let liveApps = $state<Record<string, AppStateInfo>>({});
  let apps = $derived.by(() => mergeLedgerRows(configEntries, liveApps));
  let appList = $derived(
    Object.values(apps).sort((a, b) =>
      a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
    ),
  );
  let tally = $derived(summarize(apps));

  /** Re-reads config.toml into the ledger. Called on mount and after every
   * config write, so the list reflects the file immediately. */
  async function refreshFromConfig() {
    try {
      const configs = await getFullConfig();
      configEntries = configs;
    } catch (e) {
      errorMessage = `Could not read the config: ${e}`;
    }
  }

  // ── Window chrome state ──────────────────────────────────────────────────
  let maximized = $state(false);

  // ── Theme ────────────────────────────────────────────────────────────────
  // The setting is light | dark | system; "system" follows the OS preference.
  let themeSetting = $state<ThemeSetting>("system");
  let prefersDark = $state(false);

  function applyCurrentTheme() {
    const theme = resolveTheme(themeSetting, prefersDark);
    applyTheme(document.documentElement, theme);
    syncWindowTheme(theme);
  }

  function toggleTheme() {
    themeSetting = nextSetting(themeSetting);
    writeSetting(localStorage, themeSetting);
    applyCurrentTheme();
  }

  // ── Editor state ─────────────────────────────────────────────────────────
  let editorOpen = $state(false);
  let isCreateMode = $state(false);
  let confirmDelete = $state(false);
  let isSaving = $state(false);
  let isPickingCommand = $state(false);
  let isPickingStopCommand = $state(false);
  let errorMessage = $state("");
  let editingOriginalName = $state("");
  let editForm = $state<AppConfig>(makeEmptyForm());

  let panelEl = $state<HTMLDivElement | null>(null);
  let nameInputEl = $state<HTMLInputElement | null>(null);
  let lastFocused = $state<HTMLElement | null>(null);

  // ── Row menu state ───────────────────────────────────────────────────────
  let rowMenu = $state({ visible: false, x: 0, y: 0, name: "", enabled: false });
  let restarting = $state<Record<string, boolean>>({});

  // ── Log panel state ──────────────────────────────────────────────────────
  // The log panel is a read-only view of one app's captured output. It opens
  // from the row menu, is independent of the editor, and keeps a rolling buffer
  // per app so switching between apps does not lose history.
  let logPanelOpen = $state(false);
  let logAppName = $state("");
  let logEvents = $state<LogEvent[]>([]);
  let logFollow = $state(true);
  let logScrollerEl = $state<HTMLElement | null>(null);
  const logRowsByApp: Record<string, LogEvent[]> = {};

  function appendLogEvent(name: string, event: LogEvent) {
    const buffer = logRowsByApp[name] ?? (logRowsByApp[name] = []);
    buffer.push(event);
    // Bound the buffer so a chatty app cannot grow the UI without limit.
    if (buffer.length > 2000) buffer.splice(0, buffer.length - 2000);
    if (logPanelOpen && logAppName === name) {
      logEvents = [...buffer];
    }
  }

  // ── Drop-target state ────────────────────────────────────────────────────
  let dragActive = $state(false);

  let commandExtensions = $state<string[]>([".exe", ".cmd", ".bat", ".com", ".ps1", ".vbs"]);

  // The literal `${PID}` token, referenced from markup without triggering
  // Svelte's expression parser.
  const PID_TOKEN = "${PID}";

  onMount(() => {
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (rowMenu.visible) closeRowMenu();
        else if (logPanelOpen) closeLogPanel();
        else if (editorOpen) closeEditor();
      }
    };
    window.addEventListener("keydown", onKeydown);

    // Follow the OS preference live while the setting is "system".
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onPrefersChange = (e: MediaQueryListEvent) => {
      prefersDark = e.matches;
      applyCurrentTheme();
    };
    media.addEventListener("change", onPrefersChange);

    void (async () => {
      commandExtensions = await getCommandExtensions();

      // Theme: apply the persisted setting before anything else paints.
      themeSetting = readSetting(localStorage);
      prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      applyCurrentTheme();

      // Seed the ledger from config.toml before any status arrives, so the
      // list is correct even when no core is attached.
      await refreshFromConfig();

      // Subscribe before fetching the backlog: the core may have pushed
      // statuses while the webview was still starting, and a status that lands
      // between these two calls must not be missed.
      await onStateUpdate((update) => {
        liveApps = applyStateUpdate(liveApps, update);
      });
      for (const line of await getInitialStatuses()) {
        try {
          liveApps = applyStateUpdate(liveApps, JSON.parse(line));
        } catch (e) {
          console.error("Failed to parse a cached status:", e);
        }
      }

      // Log events arrive on their own stream, keyed by app name.
      await onLogUpdate((update) => {
        appendLogEvent(update.name, update.event);
      });

      await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === "over") {
          dragActive = true;
        } else if (event.payload.type === "drop") {
          dragActive = false;
          handleDroppedPaths(event.payload.paths);
        } else {
          dragActive = false;
        }
      });

      maximized = await isWindowMaximized();
      await onWindowResized(async () => {
        maximized = await isWindowMaximized();
      });
    })();

    return () => {
      window.removeEventListener("keydown", onKeydown);
      media.removeEventListener("change", onPrefersChange);
    };
  });

  // ── Ledger presentation ──────────────────────────────────────────────────

  /** A disabled entry is reported as "Paused" regardless of its core state:
   * that is what the user needs to know. One concept, one word: the ledger, the
   * row menu (Pause/Resume), and the editor checkbox all say "paused". */
  function displayState(app: AppStateInfo) {
    return app.enabled ? stateView(app.state) : { label: "Paused", tone: "idle" as const, healthy: false };
  }

  // ── Log panel ────────────────────────────────────────────────────────────

  let logView = $derived<LogRow[]>(logRows(logEvents));

  /** Opens the log panel for `appName`, seeding it from the buffered history. */
  async function openLogPanel(appName: string) {
    logAppName = appName;
    logEvents = [...(logRowsByApp[appName] ?? [])];
    logFollow = true;
    logPanelOpen = true;
    // The UI's own stdin listener may already hold lines this client has not
    // applied yet (for example if the panel was closed when they arrived).
    try {
      const buffered = await getAppLogs(appName);
      if (logEvents.length === 0 && buffered.length > 0) {
        const parsed: LogEvent[] = [];
        for (const line of buffered) {
          try {
            const value = JSON.parse(line) as { event?: LogEvent };
            if (value.event) parsed.push(value.event);
          } catch {
            // Ignore a malformed buffered line.
          }
        }
        logRowsByApp[appName] = parsed;
        logEvents = [...parsed];
      }
    } catch (e) {
      console.error("Failed to load the log backlog:", e);
    }
    await tick();
    scrollLogToEnd();
  }

  function closeLogPanel() {
    logPanelOpen = false;
    logAppName = "";
  }

  /** Keeps the view pinned to the newest line while follow mode is on. */
  function scrollLogToEnd() {
    if (!logFollow) return;
    const el = logScrollerEl;
    if (el) el.scrollTop = el.scrollHeight;
  }

  /** Called after each render; keeps the newest line in view when following. */
  function onLogScroll() {
    const el = logScrollerEl;
    if (!el) return;
    // Leaving the bottom turns follow off, so a user reading history is not
    // yanked back down by new lines; returning to the bottom re-arms it.
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    logFollow = atBottom;
  }

  // Pin the view to the newest line after each render while following.
  $effect(() => {
    void logView.length;
    scrollLogToEnd();
  });

  function openEditor(appName: string) {
    errorMessage = "";
    confirmDelete = false;
    isSaving = false;
    isCreateMode = false;
    getFullConfig()
      .then((configs) => {
        const cfg = configs[appName];
        if (!cfg) {
          errorMessage = `No entry named "${appName}" in the config.`;
          return;
        }
        editingOriginalName = appName;
        editForm = { ...cfg, env: cfg.env.map((pair) => ({ ...pair })) };
        showEditor();
      })
      .catch((e) => {
        errorMessage = `Could not read the config: ${e}`;
      });
  }

  function openCreate() {
    errorMessage = "";
    confirmDelete = false;
    isSaving = false;
    isCreateMode = true;
    editingOriginalName = "";
    editForm = makeEmptyForm();
    showEditor();
  }

  async function showEditor() {
    lastFocused = document.activeElement as HTMLElement | null;
    editorOpen = true;
    await tick();
    nameInputEl?.focus();
  }

  function closeEditor() {
    editorOpen = false;
    confirmDelete = false;
    errorMessage = "";
    isCreateMode = false;
    // Return focus to whatever opened the editor.
    lastFocused?.focus?.();
    lastFocused = null;
  }

  // ── Environment rows ──────────────────────────────────────────────────────

  /** Appends an empty pair and focuses its name field on the next tick. */
  async function addEnvRow() {
    const index = editForm.env.length;
    editForm.env = [...editForm.env, { key: "", value: "" }];
    await tick();
    panelEl?.querySelector<HTMLInputElement>(`[data-env-key="${index}"]`)?.focus();
  }

  function removeEnvRow(index: number) {
    editForm.env = editForm.env.filter((_, i) => i !== index);
  }

  /** Keeps Tab focus inside the dialog while it is open. */
  function trapFocus(e: KeyboardEvent) {
    if (e.key !== "Tab") return;
    const root = panelEl;
    if (!root) return;
    const focusable = root.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [href], [tabindex]:not([tabindex="-1"])',
    );
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  // ── Row menu ─────────────────────────────────────────────────────────────

  function openRowMenu(e: MouseEvent, app: AppStateInfo) {
    e.preventDefault();
    const w = 180;
    const h = 156;
    rowMenu = {
      visible: true,
      x: Math.min(e.clientX, window.innerWidth - w - 10),
      y: Math.min(e.clientY, window.innerHeight - h - 10),
      name: app.name,
      enabled: app.enabled,
    };
  }

  function closeRowMenu() {
    rowMenu = { ...rowMenu, visible: false };
  }

  async function toggleEnabled(appName: string, target: boolean) {
    closeRowMenu();
    try {
      const configs = await getFullConfig();
      const cfg = configs[appName];
      if (!cfg) return;
      await updateAppConfig(appName, { ...cfg, enabled: target });
      await refreshFromConfig();
    } catch (e) {
      errorMessage = `Could not ${target ? "resume" : "pause"} "${appName}": ${e}`;
    }
  }

  async function restartApp(appName: string) {
    closeRowMenu();
    if (restarting[appName]) return;
    restarting = { ...restarting, [appName]: true };
    try {
      const configs = await getFullConfig();
      const cfg = configs[appName];
      if (!cfg) return;
      // Pause, let the core's debounce stop it, then resume.
      await updateAppConfig(appName, { ...cfg, enabled: false });
      await refreshFromConfig();
      await new Promise((r) => setTimeout(r, 900));
      await updateAppConfig(appName, { ...cfg, enabled: true });
      await refreshFromConfig();
    } catch (e) {
      errorMessage = `Could not restart "${appName}": ${e}`;
    } finally {
      const next = { ...restarting };
      delete next[appName];
      restarting = next;
    }
  }

  // ── Command selection / drop ─────────────────────────────────────────────

  function setCommandValue(command: string) {
    editForm = {
      ...editForm,
      command,
      name: editForm.name.trim() ? editForm.name : extractAppNameFromPath(command),
    };
  }

  function handleDroppedPaths(paths: string[]) {
    const dropped = (paths[0] ?? "").trim();
    if (!dropped) return;
    if (!isSupportedCommandFile(dropped, commandExtensions)) {
      errorMessage =
        "That file is not a runnable command. Drop an executable or script (.exe, .cmd, .bat, .ps1…).";
      return;
    }
    errorMessage = "";
    if (!editorOpen) openCreate();
    setCommandValue(dropped);
  }

  async function browseCommand() {
    isPickingCommand = true;
    errorMessage = "";
    try {
      const selected = await selectCommandPath(editForm.command);
      if (selected?.trim()) setCommandValue(selected.trim());
    } catch (e) {
      errorMessage = `Could not open the file picker: ${e}`;
    } finally {
      isPickingCommand = false;
    }
  }

  async function browseStopCommand() {
    isPickingStopCommand = true;
    errorMessage = "";
    try {
      const selected = await selectCommandPath(editForm.stopCommand);
      if (selected?.trim()) editForm = { ...editForm, stopCommand: selected.trim() };
    } catch (e) {
      errorMessage = `Could not open the file picker: ${e}`;
    } finally {
      isPickingStopCommand = false;
    }
  }

  // ── Save / delete ────────────────────────────────────────────────────────

  async function save() {
    const name = editForm.name.trim();
    const command = editForm.command.trim();
    if (!name) {
      errorMessage = "Give the app a name.";
      return;
    }
    if (!command) {
      errorMessage = "A command is required — pick or type the executable to run.";
      return;
    }

    const unnamed = editForm.env.some((pair) => !pair.key.trim() && pair.value.trim() !== "");
    if (unnamed) {
      errorMessage = "Give every environment variable a name before the \"=\".";
      return;
    }
    const env = normalizeEnv(editForm.env);

    isSaving = true;
    errorMessage = "";
    try {
      if (isCreateMode) {
        const configs = await getFullConfig();
        if (configs[name]) {
          errorMessage = `"${name}" already exists. Choose another name.`;
          return;
        }
      }
      await updateAppConfig(editingOriginalName, { ...editForm, name, command, env });
      await refreshFromConfig();
      closeEditor();
    } catch (e) {
      errorMessage = `Could not save: ${e}`;
    } finally {
      isSaving = false;
    }
  }

  async function confirmDeleteApp() {
    isSaving = true;
    errorMessage = "";
    try {
      await deleteAppConfig(editingOriginalName);
      await refreshFromConfig();
      closeEditor();
    } catch (e) {
      errorMessage = `Could not delete: ${e}`;
    } finally {
      isSaving = false;
    }
  }
</script>

<svelte:window />

<main class="window">
  <Titlebar {maximized} {tally} onAdd={openCreate} themeSetting={themeSetting} onToggleTheme={toggleTheme} />

  <div class="body">
    {#if appList.length === 0}
      <div class="empty">
        <h1>Nothing supervised yet</h1>
        <p>
          Add an app, or drop an executable onto this window. Kandel runs it in your
          session and brings it back if it dies.
        </p>
        <button class="ghost" type="button" onclick={openCreate}>Add your first app</button>
      </div>
    {:else}
      <div class="ledger-wrap">
        <table class="ledger">
          <thead>
            <tr>
              <th class="c-app">App</th>
              <th class="c-state">State</th>
              <th class="c-pid num">PID</th>
              <th class="c-restarts num">Restarts</th>
            </tr>
          </thead>
          <tbody>
            {#each appList as app (app.name)}
              {@const view = displayState(app)}
              <tr
                ondblclick={() => openEditor(app.name)}
                oncontextmenu={(e) => openRowMenu(e, app)}
                class:disabled={!app.enabled}
              >
                <td class="c-app">
                  <div class="app-cell">
                    <span class="charge charge-{view.tone}" aria-hidden="true"></span>
                    <span class="app-text">
                      <span class="app-name">{app.name}</span>
                      <span class="app-cmd selectable" title={formatCommandLine(app)}>{formatCommandLine(app)}</span>
                    </span>
                  </div>
                </td>
                <td class="c-state">
                  <span class="state state-{view.tone}">{view.label}</span>
                </td>
                <td class="c-pid num tnum">{app.pid > 0 ? app.pid : "—"}</td>
                <td class="c-restarts num tnum">
                  {#if app.restartCount > 0}
                    <span class="restarted">{app.restartCount}</span>
                  {:else}
                    <span class="muted">0</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>

  <footer class="foot">
    <span>Double-click a row to edit · right-click for actions</span>
  </footer>
</main>

<!-- Whole-window drop affordance -->
{#if dragActive}
  <div class="drop-veil" aria-hidden="true">
    <div class="drop-card">
      <span class="charge charge-alive"></span>
      <strong>Drop to add</strong>
      <span>Release an executable or script to register it.</span>
    </div>
  </div>
{/if}

<ResizeHandles />

<!-- Row actions -->
{#if rowMenu.visible}
  <div
    class="scrim"
    onclick={closeRowMenu}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeRowMenu();
    }}
    role="presentation"
  ></div>
  <div class="rowmenu" style="top: {rowMenu.y}px; left: {rowMenu.x}px;" role="menu">
    <div class="rowmenu-head">{rowMenu.name}</div>
    {#if rowMenu.enabled}
      <button class="rowmenu-item" role="menuitem" onclick={() => restartApp(rowMenu.name)}>
        {restarting[rowMenu.name] ? "Restarting…" : "Restart"}
      </button>
      <button class="rowmenu-item" role="menuitem" onclick={() => toggleEnabled(rowMenu.name, false)}>
        Pause
      </button>
    {:else}
      <button class="rowmenu-item" role="menuitem" onclick={() => toggleEnabled(rowMenu.name, true)}>
        Resume
      </button>
    {/if}
    <div class="rowmenu-sep"></div>
    <button
      class="rowmenu-item"
      role="menuitem"
      onclick={() => {
        closeRowMenu();
        openLogPanel(rowMenu.name);
      }}
    >
      View log…
    </button>
    <button
      class="rowmenu-item"
      role="menuitem"
      onclick={() => {
        closeRowMenu();
        openEditor(rowMenu.name);
      }}
    >
      Edit…
    </button>
  </div>
{/if}

<!-- Editor -->
{#if editorOpen}
  <div class="scrim" onclick={closeEditor} role="presentation"></div>
  <div
    class="panel"
    bind:this={panelEl}
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="editor-title"
    onkeydown={trapFocus}
  >
    <header class="panel-head">
      <h2 id="editor-title">{isCreateMode ? "Add app" : editingOriginalName}</h2>
      <button class="panel-x" type="button" aria-label="Close" onclick={closeEditor}>
        <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
          <path d="M1 1l10 10M11 1L1 11" stroke="currentColor" stroke-width="1.4" />
        </svg>
      </button>
    </header>

    {#if confirmDelete}
      <div class="panel-body confirm">
        <p>
          Delete <strong>{editingOriginalName}</strong>? Kandel stops it and removes it from
          the config. This cannot be undone.
        </p>
        {#if errorMessage}
          <p class="error" role="alert">{errorMessage}</p>
        {/if}
      </div>
      <footer class="panel-foot">
        <button class="ghost" type="button" onclick={() => (confirmDelete = false)} disabled={isSaving}>
          Keep it
        </button>
        <span class="grow"></span>
        <button class="danger" type="button" onclick={confirmDeleteApp} disabled={isSaving}>
          {isSaving ? "Deleting…" : "Delete app"}
        </button>
      </footer>
    {:else}
      <div class="panel-body">
        <form
          class="form"
          onsubmit={(e) => {
            e.preventDefault();
            save();
          }}
        >
          <label class="field span2">
            <span class="lbl">Name</span>
            <input class="inp" bind:this={nameInputEl} bind:value={editForm.name} placeholder="My dev server" />
          </label>

          <div class="field span2">
            <span class="lbl">Command</span>
            <div class="with-btn">
              <input class="inp" bind:value={editForm.command} placeholder="C:\Program Files\App\app.exe" />
              <button class="ghost sm" type="button" onclick={browseCommand} disabled={isSaving || isPickingCommand}>
                {isPickingCommand ? "…" : "Browse"}
              </button>
            </div>
          </div>

          <label class="field span2">
            <span class="lbl">Arguments <em>argv, not shell — quoting is respected</em></span>
            <input class="inp" bind:value={editForm.args} placeholder='/c "echo hello" --port 3000' />
          </label>

          <label class="field span2">
            <span class="lbl">Working directory <em>optional</em></span>
            <input class="inp" bind:value={editForm.cwd} placeholder="Defaults to the command's folder" />
          </label>

          <div class="divider span2">
            <span>Environment <em>optional</em></span>
          </div>

          <div class="field span2">
            <span class="lbl">
              Variables <em>added for this app; values may use {"${VAR}"}</em>
            </span>
            {#if editForm.env.length === 0}
              <p class="env-empty">No variables. The app inherits your environment.</p>
            {:else}
              <ul class="env-list">
                {#each editForm.env as row, i (i)}
                  <li class="env-row">
                    <input
                      class="inp env-key"
                      data-env-key={i}
                      value={row.key}
                      oninput={(e) => {
                        const el = e.currentTarget as HTMLInputElement;
                        const clean = sanitizeEnvKey(el.value);
                        // Fully controlled: the model holds the sanitized name
                        // and the field is written back every keystroke, so the
                        // space-to-underscore replacement is visible live and
                        // cannot be undone by a stray binding write.
                        row.key = clean;
                        if (el.value !== clean) el.value = clean;
                      }}
                      placeholder="NAME"
                      aria-label="Environment variable {i + 1} name"
                      autocapitalize="off"
                      autocorrect="off"
                      autocomplete="off"
                      spellcheck="false"
                    />
                    <span class="eq" aria-hidden="true">=</span>
                    <input
                      class="inp env-val"
                      bind:value={row.value}
                      placeholder="value"
                      aria-label="Environment variable {i + 1} value"
                      autocapitalize="off"
                      spellcheck="false"
                    />
                    <button
                      class="env-del"
                      type="button"
                      aria-label="Remove variable {row.key.trim() || i + 1}"
                      onclick={() => removeEnvRow(i)}
                    >
                      <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
                        <path d="M1 1l10 10M11 1L1 11" stroke="currentColor" stroke-width="1.4" />
                      </svg>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
            <button class="ghost sm env-add" type="button" onclick={addEnvRow}>Add variable</button>
          </div>

          <div class="checks span2">
            <label class="check">
              <input type="checkbox" bind:checked={editForm.enabled} />
              <span>Paused</span>
            </label>
            <label class="check">
              <input type="checkbox" bind:checked={editForm.hideWindow} />
              <span>Run hidden</span>
            </label>
            <label class="check">
              <input type="checkbox" bind:checked={editForm.captureOutput} />
              <span>Capture output</span>
            </label>
          </div>

          <div class="divider span2"><span>Restart policy</span></div>

          <label class="field">
            <span class="lbl">Delay <em>seconds</em></span>
            <input
              class="inp"
              type="number"
              min="0"
              max="3600"
              bind:value={editForm.restartDelaySec}
              onchange={() => (editForm.restartDelaySec = clampInt(editForm.restartDelaySec, 0, 3600))}
            />
          </label>
          <label class="field">
            <span class="lbl">Max retries <em>0 none · -1 ∞</em></span>
            <input
              class="inp"
              type="number"
              min="-1"
              max="999"
              bind:value={editForm.maxRetries}
              onchange={() => (editForm.maxRetries = clampInt(editForm.maxRetries, -1, 999))}
            />
          </label>
          <label class="field">
            <span class="lbl">Healthy timeout <em>seconds</em></span>
            <input
              class="inp"
              type="number"
              min="0"
              max="3600"
              bind:value={editForm.healthyTimeoutSec}
              onchange={() => (editForm.healthyTimeoutSec = clampInt(editForm.healthyTimeoutSec, 0, 3600))}
            />
          </label>
          <label class="field">
            <span class="lbl">Stop timeout <em>seconds</em></span>
            <input
              class="inp"
              type="number"
              min="0"
              max="3600"
              bind:value={editForm.stopTimeoutSec}
              onchange={() => (editForm.stopTimeoutSec = clampInt(editForm.stopTimeoutSec, 0, 3600))}
            />
          </label>

          <div class="divider span2"><span>Graceful stop <em>optional</em></span></div>

          <div class="field span2">
            <span class="lbl">Stop command</span>
            <div class="with-btn">
              <input class="inp" bind:value={editForm.stopCommand} placeholder="Leave empty to close the window or send Ctrl-Break" />
              <button class="ghost sm" type="button" onclick={browseStopCommand} disabled={isSaving || isPickingStopCommand}>
                {isPickingStopCommand ? "…" : "Browse"}
              </button>
            </div>
          </div>

          <label class="field span2">
            <span class="lbl">Stop arguments <em>{"${PID}"} is the app's PID</em></span>
            <input class="inp" bind:value={editForm.stopArgs} placeholder={"/PID " + PID_TOKEN + " /T"} />
          </label>

          {#if errorMessage}
            <p class="error span2" role="alert">{errorMessage}</p>
          {/if}

          <button class="submit-hidden" type="submit" tabindex="-1" aria-hidden="true"></button>
        </form>
      </div>

      <footer class="panel-foot">
        {#if !isCreateMode}
          <button class="ghost danger-text" type="button" onclick={() => (confirmDelete = true)} disabled={isSaving}>
            Delete
          </button>
        {/if}
        <span class="grow"></span>
        <button class="ghost" type="button" onclick={closeEditor} disabled={isSaving}>Cancel</button>
        <button class="primary" type="button" onclick={save} disabled={isSaving}>
          {isSaving ? "Saving…" : isCreateMode ? "Add app" : "Save changes"}
        </button>
      </footer>
    {/if}
  </div>
{/if}

<!-- Log panel -->
{#if logPanelOpen}
  <div class="scrim" onclick={closeLogPanel} role="presentation"></div>
  <div
    class="panel panel-log"
    role="dialog"
    aria-modal="true"
    aria-labelledby="log-title"
  >
    <header class="panel-head">
      <h2 id="log-title"><span class="log-app">{logAppName}</span> log</h2>
      <button class="panel-x" type="button" aria-label="Close" onclick={closeLogPanel}>
        <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
          <path d="M1 1l10 10M11 1L1 11" stroke="currentColor" stroke-width="1.4" />
        </svg>
      </button>
    </header>

    <div class="log-wrap">
      {#if logView.length === 0}
        <p class="log-empty">
          Nothing captured yet. Output appears here once the app writes to stdout or
          stderr.
        </p>
      {:else}
        <!-- A scrollable region must be reachable by keyboard; the tabindex is
             the documented a11y remedy, which svelte-check cannot infer. -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div
          class="log-scroll"
          bind:this={logScrollerEl}
          onscroll={onLogScroll}
          role="log"
          aria-label="{logAppName} output"
          tabindex="0"
        >
          {#each logView as row, i (i)}
            {#if row.kind === "divider"}
              <div class="log-session">
                <span class="tnum">{formatClock(row.ts)}</span>
                <span class="log-session-label">process started</span>
              </div>
            {:else}
              <div class="log-line" class:is-err={row.stream === "stderr"}>
                <span class="log-ts tnum">{formatClock(row.ts)}</span>
                <span class="log-stream">{row.stream === "stderr" ? "err" : "out"}</span>
                <span class="log-text selectable">{row.text}</span>
              </div>
            {/if}
          {/each}
        </div>
      {/if}
    </div>

    <footer class="panel-foot">
      <label class="check">
        <input type="checkbox" bind:checked={logFollow} onchange={scrollLogToEnd} />
        <span>Follow</span>
      </label>
      <span class="grow"></span>
      <button class="ghost" type="button" onclick={closeLogPanel}>Close</button>
    </footer>
  </div>
{/if}

<style>
  /* ── Shell ──────────────────────────────────────────────────────────────── */
  .window {
    display: flex;
    flex-direction: column;
    height: 100vh;
    /* Page canvas: the clean white base (reference surface level 0). */
    background: var(--canvas);
    overflow: hidden;
  }

  .body {
    flex: 1;
    min-height: 0;
    padding: var(--s-24);
    overflow: hidden;
    display: flex;
  }

  .foot {
    padding: 0 var(--s-24) var(--s-16);
    font-size: var(--text-caption);
    font-weight: var(--w-regular);
    color: var(--graphite);
    text-align: right;
  }

  /* ── Ledger ─────────────────────────────────────────────────────────────── */
  .ledger-wrap {
    flex: 1;
    min-height: 0;
    overflow: auto;
    /* The ledger is a parchment card on the white canvas (reference surface
       level 1): the warm print surface, lifted by tonal contrast, no shadow. */
    background: var(--parchment);
    border-radius: var(--r-card);
    border: 1px solid var(--ash);
  }

  .ledger {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-body-sm);
  }

  .ledger thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--parchment);
    text-align: left;
    padding: var(--s-16) var(--s-20);
    font-size: var(--text-eyebrow);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
    border-bottom: 1px solid var(--ink);
    white-space: nowrap;
  }
  .ledger thead th.num {
    text-align: right;
  }

  .ledger tbody td {
    padding: var(--s-16) var(--s-20);
    border-bottom: 1px solid var(--ash);
    vertical-align: middle;
  }
  .ledger tbody tr:last-child td {
    border-bottom: none;
  }
  .ledger tbody tr {
    transition: background 0.12s var(--ease-out);
  }
  .ledger tbody tr:hover {
    /* Lifting a row means a lighter paper, not a shadow. In light mode that is
       the white canvas; in dark the token resolves to a lifted warm wash. */
    background: var(--surface-hover);
  }
  .ledger tbody tr.disabled .app-name {
    color: var(--graphite);
  }

  .c-app {
    width: 55%;
  }
  .c-state {
    width: 1%;
    white-space: nowrap;
  }
  .c-pid,
  .c-restarts {
    width: 1%;
    white-space: nowrap;
    text-align: right;
    color: var(--graphite);
  }

  .app-cell {
    display: flex;
    align-items: center;
    gap: var(--s-16);
    min-width: 0;
  }
  .app-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .app-name {
    font-size: var(--text-body-sm);
    font-weight: var(--w-medium);
    color: var(--ink);
    letter-spacing: -0.01em;
  }
  .app-cmd {
    font-size: var(--text-caption);
    color: var(--graphite);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 46ch;
  }

  /* The state charge: a small filled marker. Lime is the only "alive" colour;
     amber and red appear only where they carry meaning (a degraded or failed
     app), as the Operate mode allows. The marker uses a real border, never a
     shadow: the world separates layers by tone alone. The state word carries
     the meaning, so nothing depends on colour alone. */
  .charge {
    flex: none;
    box-sizing: border-box;
    width: 10px;
    height: 10px;
    border-radius: 3px;
    background: var(--smoke);
    border: 1px solid var(--charge-ring);
  }
  .charge-alive {
    background: var(--lime);
  }
  .charge-waiting {
    background: var(--degraded);
  }
  .charge-down {
    background: var(--alert);
  }
  .charge-idle {
    background: transparent;
    border-color: var(--stone);
  }

  .state {
    font-size: var(--text-caption);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
  }
  .state-alive {
    color: var(--ink);
  }
  .state-waiting {
    color: var(--degraded);
  }
  .state-down {
    color: var(--alert);
  }
  .state-idle {
    color: var(--graphite);
  }

  .restarted {
    color: var(--alert);
    font-weight: var(--w-medium);
  }
  .muted {
    /* Stone is documented for strokes, not text; use graphite so the count
       still clears AA on parchment. */
    color: var(--graphite);
  }

  /* ── Empty state ────────────────────────────────────────────────────────── */
  .empty {
    margin: auto;
    max-width: 42ch;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--s-16);
    padding: var(--s-48);
  }
  .empty h1 {
    margin: 0;
    font-size: var(--text-heading-lg);
    font-weight: var(--w-medium);
    letter-spacing: var(--tracking-tight);
    line-height: 0.95;
    max-width: 14ch;
  }
  .empty p {
    margin: 0;
    color: var(--graphite);
    line-height: 1.55;
  }

  /* ── Shared controls ────────────────────────────────────────────────────── */
  .primary,
  .ghost,
  .danger {
    font-family: inherit;
    font-size: var(--text-body-sm);
    font-weight: var(--w-medium);
    border-radius: var(--r-pill);
    padding: 0 var(--s-20);
    height: 38px;
    cursor: pointer;
    transition: background 0.15s var(--ease-out), color 0.15s var(--ease-out);
  }
  .primary {
    border: none;
    background: var(--lime);
    color: var(--lime-ink);
  }
  .primary:hover:not(:disabled) {
    filter: brightness(0.95);
  }
  .ghost {
    border: 1px solid var(--ash);
    background: transparent;
    color: var(--ink);
  }
  .ghost:hover:not(:disabled) {
    /* Controls sit on any surface, so their hover is a translucent wash that
       reads in both themes rather than a fixed surface colour. */
    background: var(--wash);
    border-color: var(--stone);
  }
  .ghost.sm {
    height: 40px;
    padding: 0 var(--s-16);
    flex: none;
  }
  .danger {
    border: none;
    background: var(--alert);
    color: #fff;
  }
  .danger:hover:not(:disabled) {
    filter: brightness(0.94);
  }
  .danger-text {
    color: var(--alert);
  }
  .primary:disabled,
  .ghost:disabled,
  .danger:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .submit-hidden {
    position: absolute;
    width: 0;
    height: 0;
    opacity: 0;
    pointer-events: none;
  }

  /* ── Row menu ───────────────────────────────────────────────────────────── */
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: var(--scrim);
  }

  .rowmenu {
    position: fixed;
    z-index: 50;
    min-width: 180px;
    background: var(--canvas);
    border: 1px solid var(--ink);
    border-radius: var(--r-inner);
    padding: var(--s-4);
  }
  .rowmenu-head {
    padding: var(--s-8) var(--s-12);
    font-size: var(--text-caption);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
    border-bottom: 1px solid var(--ash);
    margin-bottom: var(--s-4);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rowmenu-item {
    display: block;
    width: 100%;
    text-align: left;
    border: none;
    background: transparent;
    color: var(--ink);
    font-family: inherit;
    font-size: var(--text-body-sm);
    font-weight: var(--w-regular);
    padding: var(--s-8) var(--s-12);
    border-radius: var(--r-input);
    cursor: pointer;
  }
  .rowmenu-item:hover {
    background: var(--lime);
    color: var(--lime-ink);
  }
  .rowmenu-sep {
    height: 1px;
    background: var(--ash);
    margin: var(--s-4) 0;
  }

  /* ── Drop veil ──────────────────────────────────────────────────────────── */
  .drop-veil {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    background: var(--veil);
    pointer-events: none;
  }
  .drop-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--s-8);
    padding: var(--s-40) var(--s-48);
    background: var(--canvas);
    border: 2px solid var(--ink);
    border-radius: var(--r-card);
  }
  .drop-card strong {
    font-size: var(--text-subheading);
    font-weight: var(--w-medium);
    letter-spacing: var(--tracking-tight);
  }
  .drop-card span:last-child {
    color: var(--graphite);
    font-size: var(--text-body-sm);
  }

  /* ── Editor panel ───────────────────────────────────────────────────────── */
  .panel {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 70;
    width: min(720px, calc(100vw - 2 * var(--s-32)));
    max-height: calc(100vh - 2 * var(--s-32));
    display: flex;
    flex-direction: column;
    background: var(--canvas);
    border: 1.5px solid var(--ink);
    border-radius: var(--r-card);
    overflow: hidden;
    animation: rise 0.2s var(--ease-out);
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translate(-50%, calc(-50% + 14px));
    }
    to {
      opacity: 1;
      transform: translate(-50%, -50%);
    }
  }

  .panel-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-16);
    padding: var(--s-20) var(--s-24);
    border-bottom: 1px solid var(--ash);
    flex: none;
  }
  .panel-head h2 {
    margin: 0;
    font-size: var(--text-subheading);
    font-weight: var(--w-medium);
    letter-spacing: var(--tracking-tight);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .panel-x {
    flex: none;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border: 1px solid var(--ash);
    border-radius: var(--r-pill);
    background: transparent;
    color: var(--graphite);
    cursor: pointer;
  }
  .panel-x:hover {
    background: var(--wash);
    color: var(--ink);
  }

  .panel-body {
    padding: var(--s-24);
    /* Vertical scroll only: the form must never scroll sideways. */
    overflow-y: auto;
    overflow-x: hidden;
    flex: 1;
    min-height: 0;
  }
  .confirm p {
    margin: 0;
    font-size: var(--text-body);
    color: var(--ink);
    line-height: 1.55;
  }

  .panel-foot {
    display: flex;
    align-items: center;
    gap: var(--s-12);
    padding: var(--s-16) var(--s-24);
    border-top: 1px solid var(--ash);
    background: var(--canvas);
    flex: none;
  }
  .grow {
    flex: 1;
  }

  /* ── Form ───────────────────────────────────────────────────────────────── */
  .form {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--s-16);
  }
  .span2 {
    grid-column: span 2;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--s-8);
    min-width: 0;
  }
  .lbl {
    font-size: var(--text-body-sm);
    font-weight: var(--w-medium);
    color: var(--ink);
  }
  .lbl em {
    font-style: normal;
    font-weight: var(--w-regular);
    color: var(--graphite);
    font-size: var(--text-caption);
  }

  .inp {
    width: 100%;
    height: 40px;
    padding: 0 var(--s-12);
    /* Inset fields sit one step below their surface: white in light mode,
       darker than the card in dark mode. */
    background: var(--well);
    border: 1px solid var(--ash);
    border-radius: var(--r-input);
    color: var(--ink);
    font-family: inherit;
    font-size: var(--text-body-sm);
    transition: border-color 0.15s var(--ease-out);
  }
  .inp::placeholder {
    color: var(--smoke);
  }
  .inp:focus {
    outline: none;
    border-color: var(--ink);
  }

  /* Native number spinners belong to the design rather than being left at UA
     defaults, but the stepper itself stays: these are small integer policy
     fields where click-to-adjust is a real affordance on an Operate screen. */
  .inp[type="number"] {
    accent-color: var(--ink);
  }

  .with-btn {
    display: flex;
    gap: var(--s-8);
  }
  .with-btn .inp {
    flex: 1;
  }

  .checks {
    display: flex;
    gap: var(--s-24);
    padding-top: var(--s-4);
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--s-8);
    font-size: var(--text-body-sm);
    color: var(--ink);
    cursor: pointer;
  }
  .check input {
    width: 16px;
    height: 16px;
    accent-color: var(--ink);
  }

  .divider {
    display: flex;
    align-items: center;
    gap: var(--s-12);
    margin-top: var(--s-8);
    font-size: var(--text-eyebrow);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
  }
  .divider em {
    font-style: normal;
    text-transform: none;
    letter-spacing: 0;
    font-weight: var(--w-regular);
  }
  .divider::after {
    content: "";
    flex: 1;
    height: 1px;
    background: var(--ash);
  }

  .error {
    margin: 0;
    padding: var(--s-12) var(--s-16);
    background: var(--alert-wash);
    border: 1px solid var(--alert);
    border-radius: var(--r-input);
    color: var(--alert);
    font-size: var(--text-body-sm);
  }

  /* ── Environment rows ───────────────────────────────────────────────────── */
  .env-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--s-8);
  }
  .env-row {
    display: grid;
    /* Key, `=`, value, delete. The inputs can shrink below their content. */
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1.4fr) auto;
    align-items: center;
    gap: var(--s-8);
  }
  .env-row .eq {
    color: var(--graphite);
    font-size: var(--text-body-sm);
  }
  /* The name is the variable's identity; the value is its payload. */
  .env-key {
    font-weight: var(--w-medium);
  }
  .env-del {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border: 1px solid var(--ash);
    border-radius: var(--r-pill);
    background: transparent;
    color: var(--graphite);
    cursor: pointer;
    transition: background 0.12s var(--ease-out), color 0.12s var(--ease-out);
  }
  .env-del:hover {
    background: var(--wash);
    color: var(--ink);
  }
  .env-empty {
    margin: 0;
    color: var(--graphite);
    font-size: var(--text-caption);
  }
  .env-add {
    align-self: flex-start;
  }

  /* ── Log panel ──────────────────────────────────────────────────────────── */
  /* Wider than the editor: log lines are long and the panel is a reading
     surface, not a form. Same chrome, so it stays one vocabulary. */
  .panel-log {
    width: min(880px, calc(100vw - 2 * var(--s-32)));
  }
  .log-app {
    /* The app name is the panel's identity; the word "log" stays secondary. */
    font-weight: var(--w-medium);
  }

  .log-wrap {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .log-scroll {
    flex: 1;
    min-height: 0;
    margin: var(--s-16) var(--s-24);
    padding: var(--s-8) 0;
    background: var(--well);
    border: 1px solid var(--ash);
    border-radius: var(--r-inner);
    overflow-y: auto;
    overflow-x: hidden;
    font-size: var(--text-caption);
    line-height: 1.5;
  }

  .log-empty {
    margin: 0;
    padding: var(--s-24);
    color: var(--graphite);
    font-size: var(--text-body-sm);
  }

  .log-session {
    display: flex;
    align-items: center;
    gap: var(--s-8);
    margin: var(--s-12) var(--s-16) var(--s-4);
    color: var(--graphite);
    font-size: var(--text-eyebrow);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
  }
  .log-session::after {
    content: "";
    flex: 1;
    height: 1px;
    background: var(--ash);
  }

  .log-line {
    display: grid;
    /* Timestamp, stream tag, text. Tabular figures keep the gutter aligned. */
    grid-template-columns: 64px 28px minmax(0, 1fr);
    gap: var(--s-8);
    padding: 2px var(--s-16);
  }
  .log-line:hover {
    background: var(--wash);
  }
  .log-ts {
    color: var(--stone);
    white-space: nowrap;
  }
  .log-stream {
    color: var(--graphite);
    font-size: var(--text-eyebrow);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    padding-top: 2px;
  }
  /* stderr is meaning-bearing state colour, as the design permits. */
  .log-line.is-err .log-stream,
  .log-line.is-err .log-text {
    color: var(--alert);
  }
  .log-text {
    color: var(--ink);
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
