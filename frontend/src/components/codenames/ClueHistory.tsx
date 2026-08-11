// Clue-history sidebar: one block per team turn, each with the clue the
// spymaster gave, the guesses it bought (and what they cost), and the 💭
// private reasoning behind each decision — the same ThoughtRecord disclosure
// the Catan and Secret Hitler replays use. Entries are clickable and seek the
// step slider.

import type { ReactNode } from "react";
import type { CardIdentity, Seat } from "../../api/codenames";
import {
	identityKey,
	identityLabel,
	type ThoughtEntry,
	type TurnBlock,
} from "./replay";

/** Outcome glyph for a guess, paired with the identity label in the text. */
function outcome(id: CardIdentity, hit: boolean): string {
	if (id.kind === "assassin") return "☠";
	if (id.kind === "bystander") return "○";
	return hit ? "✓" : "✕";
}

interface Props {
	turns: TurnBlock[];
	step: number;
	thoughts: Map<number, ThoughtEntry[]>;
	seatLabel: (seat: Seat) => string;
	onSeek: (idx: number) => void;
}

export function ClueHistory({
	turns,
	step,
	thoughts,
	seatLabel,
	onSeek,
}: Props) {
	const visible = turns.filter((t) => t.startIdx < step);
	const activeTurn = visible[visible.length - 1];

	return (
		<div className="cn-panel cn-history">
			<h3>Clue history</h3>
			{visible.length === 0 && <p className="cn-dim">No clues yet.</p>}
			{visible.map((t) => (
				<section
					className={t === activeTurn ? "cn-turn cn-turn-active" : "cn-turn"}
					key={t.startIdx}
				>
					<header className={`cn-turn-head cn-side-${t.team}`}>
						<span className="cn-turn-no">Turn {t.turn}</span>
						<span className="cn-turn-team">Team {t.team.toUpperCase()}</span>
					</header>
					{t.clue && t.clueIdx !== null && t.clueIdx < step ? (
						<Line idx={t.clueIdx} onSeek={onSeek}>
							<span className="cn-clue">
								{t.clue.word} <b>{t.clue.number}</b>
							</span>
						</Line>
					) : (
						<p className="cn-dim">thinking…</p>
					)}
					<Thoughts
						at={t.clueIdx}
						step={step}
						thoughts={thoughts}
						seatLabel={seatLabel}
					/>
					<ul className="cn-guesses">
						{t.guesses
							.filter((g) => g.idx < step)
							.map((g) => (
								<li key={g.idx}>
									<Line idx={g.idx} onSeek={onSeek}>
										<span
											className={`cn-outcome cn-out-${identityKey(g.identity)}`}
										>
											{outcome(g.identity, g.hit)}
										</span>{" "}
										<span className="cn-guess-word">{g.word}</span>{" "}
										<span className="cn-dim">
											{identityLabel(g.identity)}
											{g.endsTurn ? " · turn ends" : ""}
										</span>
									</Line>
									<Thoughts
										at={g.idx}
										step={step}
										thoughts={thoughts}
										seatLabel={seatLabel}
									/>
								</li>
							))}
						{t.passedIdx !== null && t.passedIdx < step && (
							<li>
								<Line idx={t.passedIdx} onSeek={onSeek}>
									<span className="cn-dim">— passed —</span>
								</Line>
								<Thoughts
									at={t.passedIdx}
									step={step}
									thoughts={thoughts}
									seatLabel={seatLabel}
								/>
							</li>
						)}
						{t.forced
							.filter((f) => f.idx < step)
							.map((f) => (
								<li className="cn-forced" key={f.idx}>
									forced default · {seatLabel(f.seat)} · {f.decision}
								</li>
							))}
					</ul>
				</section>
			))}
		</div>
	);
}

function Line({
	idx,
	onSeek,
	children,
}: {
	idx: number;
	onSeek: (idx: number) => void;
	children: ReactNode;
}) {
	return (
		<button className="cn-seek" onClick={() => onSeek(idx + 1)} type="button">
			{children}
		</button>
	);
}

function Thoughts({
	at,
	step,
	thoughts,
	seatLabel,
}: {
	at: number | null;
	step: number;
	thoughts: Map<number, ThoughtEntry[]>;
	seatLabel: (seat: Seat) => string;
}) {
	if (at === null || at >= step) return null;
	const list = thoughts.get(at) ?? [];
	return (
		<>
			{list.map((t) => (
				<details className="cn-thought" key={`${at}-${t.seat}-${t.decision}`}>
					<summary>
						💭 {seatLabel(t.seat)}{" "}
						<span className="cn-dim">({t.decision})</span>
					</summary>
					<p>{t.text}</p>
				</details>
			))}
		</>
	);
}
