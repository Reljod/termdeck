import { ANSI_ORDER, type Theme } from "../types";

/**
 * A miniature terminal drawn in a theme's own colours.
 *
 * The sample line uses the same colours the zsh adapter assigns to the prompt,
 * so what you see here is what the prompt will actually look like rather than a
 * generic swatch grid.
 */
export function TerminalPreview({ theme, compact }: { theme: Theme; compact?: boolean }) {
  const p = theme.palette;

  return (
    <div
      className={`preview ${compact ? "preview--compact" : ""}`}
      style={{ background: p.background, color: p.foreground }}
    >
      <div className="preview__bar" style={{ background: p.surface }}>
        <span className="preview__dot" style={{ background: p.ansi.red }} />
        <span className="preview__dot" style={{ background: p.ansi.yellow }} />
        <span className="preview__dot" style={{ background: p.ansi.green }} />
        {!compact && (
          <span className="preview__title" style={{ color: p.subtle }}>
            {theme.name}
          </span>
        )}
      </div>

      <div className="preview__body">
        <div className="preview__line">
          <span style={{ color: p.accent }}>~/Developer/termdeck</span>{" "}
          <span style={{ color: p.ansi.green }}>main</span>{" "}
          <span style={{ color: p.ansi.yellow }}>±</span>
        </div>
        <div className="preview__line">
          <span style={{ color: p.ansi.green }}>❯</span>{" "}
          <span>cargo test</span>
        </div>
        {!compact && (
          <>
            <div className="preview__line">
              <span style={{ color: p.ansi.green }}>105 passed</span>
              <span style={{ color: p.subtle }}> · </span>
              <span style={{ color: p.ansi.red }}>0 failed</span>
            </div>
            <div className="preview__line">
              <span
                style={{
                  background: p.selection_background,
                  color: p.selection_foreground,
                }}
              >
                selected text
              </span>
              <span style={{ background: p.cursor, color: p.cursor_text }}> </span>
            </div>
          </>
        )}
      </div>

      <div className="preview__ansi">
        {ANSI_ORDER.map((slot) => (
          <span
            key={slot}
            className="preview__swatch"
            style={{ background: p.ansi[slot] }}
            title={slot.replace("_", " ")}
          />
        ))}
      </div>
    </div>
  );
}
