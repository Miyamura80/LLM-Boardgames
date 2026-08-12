// Clue-history sidebar: one block per team turn, each with the clue the
// spymaster gave, the guesses it bought (and what they cost), and the 💭
// private reasoning behind each decision — the same ThoughtRecord disclosure
// the Catan and Secret Hitler replays use.
//
// A turn expands to show its *own* compact board — the grid as it stood at the
// end of that turn — instead of yanking the main grid (and the slider) to that
// moment. Several turns can stay open at once, and the snapshot follows the
// spymaster overlay exactly as the main grid does.

import { useState } from "react";
import type { CardIdentity, GameRecord, Seat } from "../../api/codenames";
import {
	foldGrid,
	identityKey,
	identityLabel,
	type ThoughtEntry,
	type TurnBlock,
} from "./replay";
import { WordGrid } from "./WordGrid";

/** Outcome glyph for a guess, paired with the identity label in the text. */
function outcome(id: CardIdentity, hit: boolean): string {
	if (id.kind === "assassin") return "☠";
	if (id.kind === "bystander") return "○";
	return hit ? "✓" : "✕";
}

/**
 * Event count that folds in this turn's last event — clamped to the slider, so
 * a snapshot never shows the viewer a card the replay has not reached yet.
 */
function turnEnd(t: TurnBlock, step: number): number {
	const idxs = [
		t.startIdx,
		t.clueIdx ?? -1,
		t.passedIdx ?? -1,
		...t.guesses.map((g) => g.idx),
		...t.forced.map((f) => f.idx),
	];
	return Math.min(Math.max(...idxs) + 1, step);
}

interface Props {
	turns: TurnBlock[];
	step: number;
	thoughts: Map<number, ThoughtEntry[]>;
	seatLabel: (seat: Seat) => string;
	/** Source record for the per-turn board snapshots. */
	record: GameRecord;
	/** Mirrors the main grid's spymaster overlay. */
	spymaster: boolean;
}

export function ClueHistory({
	turns,
	step,
	thoughts,
	seatLabel,
	record,
	spymaster,
}: Props) {
	const visible = turns.filter((t) => t.startIdx < step);
	const activeTurn = visible[visible.length - 1];

	return (
		<div className="cn-panel cn-history">
			<h3>Clue history</h3>
			{visible.length === 0 && <p className="cn-dim">No clues yet.</p>}
			{visible.map((t) => (
				<Turn
					active={t === activeTurn}
					key={t.startIdx}
					record={record}
					seatLabel={seatLabel}
					spymaster={spymaster}
					step={step}
					thoughts={thoughts}
					turn={t}
				/>
			))}
		</div>
	);
}

function Turn({
	turn: t,
	active,
	step,
	thoughts,
	seatLabel,
	record,
	spymaster,
}: Omit<Props, "turns"> & {
	turn: TurnBlock;
	active: boolean;
}) {
	const [open, setOpen] = useState(false);
	const panelId = `cn-snap-${t.startIdx}`;

	return (
		<section className={active ? "cn-turn cn-turn-active" : "cn-turn"}>
			<header className={`cn-turn-head cn-side-${t.team}`}>
				<span className="cn-turn-no">Turn {t.turn}</span>
				<span className="cn-turn-team">Team {t.team.toUpperCase()}</span>
				<button
					aria-controls={panelId}
					aria-expanded={open}
					className="cn-snapbtn"
					onClick={() => setOpen(!open)}
					type="button"
				>
					{open ? "hide board ▴" : "show board ▾"}
				</button>
			</header>
			{/* Every 💭 sits directly above the row for the action it produced,
			    the way the HTML game report orders them. */}
			<Thoughts
				at={t.clueIdx}
				seatLabel={seatLabel}
				step={step}
				thoughts={thoughts}
			/>
			{t.clue && t.clueIdx !== null && t.clueIdx < step ? (
				<p className="cn-line">
					<span className="cn-clue">
						{t.clue.word} <b>{t.clue.number}</b>
					</span>
				</p>
			) : (
				<p className="cn-dim">thinking…</p>
			)}
			<ul className="cn-guesses">
				{t.guesses
					.filter((g) => g.idx < step)
					.map((g) => (
						<li key={g.idx}>
							<Thoughts
								at={g.idx}
								seatLabel={seatLabel}
								step={step}
								thoughts={thoughts}
							/>
							<span className="cn-line">
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
							</span>
						</li>
					))}
				{t.passedIdx !== null && t.passedIdx < step && (
					<li>
						<Thoughts
							at={t.passedIdx}
							seatLabel={seatLabel}
							step={step}
							thoughts={thoughts}
						/>
						<span className="cn-line cn-dim">— passed —</span>
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
			{open && (
				<TurnBoard
					at={turnEnd(t, step)}
					id={panelId}
					record={record}
					spymaster={spymaster}
					turn={t.turn}
				/>
			)}
		</section>
	);
}

/** The board as it stood after `at` events, shrunk to fit inside a turn block. */
function TurnBoard({
	record,
	at,
	turn,
	spymaster,
	id,
}: {
	record: GameRecord;
	at: number;
	turn: number;
	spymaster: boolean;
	id: string;
}) {
	const cards = foldGrid(record, at);
	const flipped = cards.filter((c) => c.identity !== null).length;
	return (
		<div className="cn-snap" id={id}>
			<WordGrid activeWord={null} cards={cards} compact spymaster={spymaster} />
			<p className="cn-dim">
				board after turn {turn} — {flipped} of {cards.length} cards turned over
			</p>
		</div>
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
				<details
					className="cn-thought"
					// Retries put several thoughts on one action, so the seat's
					// own stamp is part of the identity.
					key={`${at}-${t.seat}-${t.atEvent}`}
				>
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
