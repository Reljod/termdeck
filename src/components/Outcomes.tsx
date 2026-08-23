import type { Outcome, Status } from "../types";

const LABEL: Record<Status, string> = {
  applied: "Applied",
  pending: "Needs a nudge",
  skipped: "Skipped",
  failed: "Failed",
};

/**
 * What happened to each target on the last apply.
 *
 * `pending` is its own status rather than a quiet success because some of these
 * changes genuinely are not visible yet — a running shell cannot be recoloured
 * from outside it — and saying "done" would be a lie the user discovers later.
 */
export function Outcomes({
  outcomes,
  onDismiss,
}: {
  outcomes: Outcome[];
  onDismiss: () => void;
}) {
  if (outcomes.length === 0) return null;

  const counts = outcomes.reduce<Record<string, number>>((tally, outcome) => {
    tally[outcome.status] = (tally[outcome.status] ?? 0) + 1;
    return tally;
  }, {});

  const summary = (["applied", "pending", "failed", "skipped"] as Status[])
    .filter((status) => counts[status])
    .map((status) => `${counts[status]} ${LABEL[status].toLowerCase()}`)
    .join(" · ");

  return (
    <>
      <div className="outcomes__head">
        <span className="outcomes__summary">{summary}</span>
        <button
          className="outcomes__close"
          onClick={onDismiss}
          aria-label="Close results"
          title="Close results"
        >
          ×
        </button>
      </div>

      <ul className="outcomes">
        {outcomes.map((outcome) => (
        <li key={outcome.id} className={`outcome outcome--${outcome.status}`}>
          <div className="outcome__head">
            <span className="outcome__name">{outcome.name}</span>
            <span className={`badge badge--${outcome.status}`}>
              {LABEL[outcome.status]}
            </span>
          </div>

          <p className="outcome__message">{outcome.message}</p>

          {outcome.follow_up && (
            <p className="outcome__followup">{outcome.follow_up}</p>
          )}

          {outcome.changed.length > 0 && (
            <p className="outcome__files">
              {outcome.changed.map((file) => (
                <code key={file}>{file}</code>
              ))}
            </p>
          )}
          </li>
        ))}
      </ul>
    </>
  );
}
