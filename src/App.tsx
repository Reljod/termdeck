import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";

import { Outcomes } from "./components/Outcomes";
import { TerminalPreview } from "./components/TerminalPreview";
import type { ApplyOptions, Outcome, State, Theme } from "./types";

export default function App() {
  const [state, setState] = useState<State | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [outcomes, setOutcomes] = useState<Outcome[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await invoke<State>("get_state");
      setState(next);
      // Prefer what the machine actually looks like over what we last saved:
      // the user may have edited a dotfile by hand since.
      setSelected(
        (current) => current ?? next.detected_theme ?? next.settings.theme ?? next.settings.light_theme,
      );
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const theme = useMemo(
    () => state?.themes.find((candidate) => candidate.id === selected) ?? null,
    [state, selected],
  );

  // The window wears the theme being previewed, so picking one shows you the
  // result rather than a description of it.
  useEffect(() => {
    if (!theme) return;
    const root = document.documentElement;
    const p = theme.palette;
    const vars: Record<string, string> = {
      "--bg": p.background,
      "--fg": p.foreground,
      "--surface": p.surface,
      "--accent": p.accent,
      "--subtle": p.subtle,
      "--ok": p.ansi.green,
      "--warn": p.ansi.yellow,
      "--bad": p.ansi.red,
      "--selection": p.selection_background,
    };
    for (const [key, value] of Object.entries(vars)) {
      root.style.setProperty(key, value);
    }
    root.dataset.mode = theme.mode;
  }, [theme]);

  const enabled = state?.settings.enabled ?? [];
  const options = state?.settings.options ?? {
    live: true,
    claude_use_ansi: false,
    iterm_all_profiles: true,
  };

  const updateSettings = useCallback(
    async (next: Partial<State["settings"]>) => {
      if (!state) return;
      const settings = { ...state.settings, ...next };
      setState({ ...state, settings });
      try {
        await invoke("save_settings", { settings });
      } catch (cause) {
        setError(String(cause));
      }
    },
    [state],
  );

  const toggleTarget = (id: string) => {
    const next = enabled.includes(id)
      ? enabled.filter((candidate) => candidate !== id)
      : [...enabled, id];
    void updateSettings({ enabled: next });
  };

  const setOption = (key: keyof ApplyOptions, value: boolean) => {
    void updateSettings({ options: { ...options, [key]: value } });
  };

  const run = async (command: "apply_theme" | "toggle_theme") => {
    if (!state || busy) return;
    setBusy(true);
    setError(null);
    try {
      const args =
        command === "apply_theme"
          ? { themeId: selected, options, enabled }
          : { options, enabled };
      const result = await invoke<Outcome[]>(command, args);
      setOutcomes(result);
      const next = await invoke<State>("get_state");
      setState(next);
      if (command === "toggle_theme" && next.detected_theme) {
        setSelected(next.detected_theme);
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  if (!state) {
    return (
      <main className="app app--loading">
        <p>{error ?? "Reading your setup…"}</p>
      </main>
    );
  }

  const { light_theme, dark_theme } = state.settings;
  const applied = state.detected_theme;
  const nextInToggle = applied === dark_theme ? light_theme : dark_theme;
  const nextName =
    state.themes.find((candidate) => candidate.id === nextInToggle)?.name ?? nextInToggle;

  // Grouped by light and dark rather than by family. Most families have only
  // one theme, so grouping by family gave nearly every theme a row of its own
  // and turned the page into a long scroll. Light and dark is also the split
  // the toggle cares about.
  const groups: [string, Theme[]][] = [
    ["Light", state.themes.filter((candidate) => candidate.mode === "light")],
    ["Dark", state.themes.filter((candidate) => candidate.mode === "dark")],
  ];

  return (
    <main className="app">
      <header className="header">
        <div>
          <h1>TermDeck</h1>
          <p className="header__sub">
            iTerm2, tmux, zsh and Claude Code, kept on one theme.
          </p>
        </div>

        <button
          className="toggle"
          onClick={() => void run("toggle_theme")}
          disabled={busy}
        >
          <span className="toggle__label">Switch everything to</span>
          <span className="toggle__theme">{nextName}</span>
        </button>
      </header>

      {error && <p className="error">{error}</p>}

      <section className="section">
        <div className="section__head">
          <h2>Theme</h2>
          {applied && (
            <span className="section__note">
              Currently applied:{" "}
              {state.themes.find((t) => t.id === applied)?.name ?? applied}
            </span>
          )}
        </div>

        {groups.map(([label, themes]) => (
          <div key={label} className="family">
            <h3 className="family__name">
              {label} · {themes.length}
            </h3>
            <div className="grid">
              {themes.map((candidate) => (
                <button
                  key={candidate.id}
                  className={`card ${selected === candidate.id ? "card--on" : ""}`}
                  onClick={() => setSelected(candidate.id)}
                >
                  <TerminalPreview theme={candidate} compact />
                  <span className="card__foot">
                    <span className="card__name">{candidate.name}</span>
                    <span className="card__mode">{candidate.family}</span>
                  </span>
                  {applied === candidate.id && (
                    <span className="card__applied">applied</span>
                  )}
                </button>
              ))}
            </div>
          </div>
        ))}
      </section>

      {theme && (
        <section className="section">
          <h2>Preview</h2>
          <TerminalPreview theme={theme} />
        </section>
      )}

      <section className="section">
        <h2>What it changes</h2>
        <ul className="targets">
          {state.targets.map((target) => {
            const on = enabled.includes(target.id);
            return (
              <li
                key={target.id}
                className={`target ${target.installed ? "" : "target--missing"}`}
              >
                <label className="target__toggle">
                  <input
                    type="checkbox"
                    checked={on}
                    onChange={() => toggleTarget(target.id)}
                    disabled={!target.installed}
                  />
                  <span className="target__name">{target.name}</span>
                </label>
                <p className="target__detail">{target.detail}</p>
                <p className="target__paths">
                  {target.paths.map((path) => (
                    <code key={path}>{path}</code>
                  ))}
                </p>
              </li>
            );
          })}
        </ul>

        <div className="options">
          <label>
            <input
              type="checkbox"
              checked={options.live}
              onChange={(event) => setOption("live", event.target.checked)}
            />
            Recolour sessions that are already open
          </label>
          <label>
            <input
              type="checkbox"
              checked={options.claude_use_ansi}
              onChange={(event) => setOption("claude_use_ansi", event.target.checked)}
            />
            Make Claude Code use the terminal's own palette
          </label>
          <label>
            <input
              type="checkbox"
              checked={options.iterm_all_profiles}
              onChange={(event) =>
                setOption("iterm_all_profiles", event.target.checked)
              }
            />
            Apply to every iTerm2 profile, not just the default
          </label>
        </div>

        <div className="pair">
          <label>
            Light half of the toggle
            <select
              value={light_theme}
              onChange={(event) =>
                void updateSettings({ light_theme: event.target.value })
              }
            >
              {state.themes
                .filter((candidate) => candidate.mode === "light")
                .map((candidate) => (
                  <option key={candidate.id} value={candidate.id}>
                    {candidate.name}
                  </option>
                ))}
            </select>
          </label>
          <label>
            Dark half of the toggle
            <select
              value={dark_theme}
              onChange={(event) =>
                void updateSettings({ dark_theme: event.target.value })
              }
            >
              {state.themes
                .filter((candidate) => candidate.mode === "dark")
                .map((candidate) => (
                  <option key={candidate.id} value={candidate.id}>
                    {candidate.name}
                  </option>
                ))}
            </select>
          </label>
        </div>
      </section>

      <footer className="footer">
        <button
          className="apply"
          onClick={() => void run("apply_theme")}
          disabled={busy || !selected}
        >
          {busy ? "Applying…" : `Apply ${theme?.name ?? ""}`}
        </button>
        <Outcomes outcomes={outcomes} />
      </footer>
    </main>
  );
}
