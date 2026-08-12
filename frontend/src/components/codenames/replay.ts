// Pure derivations over a Codenames `GameRecord`: the grid state at a step, the
// clue-by-clue turn structure, and where private thoughts belong.
//
// The key card is engine *state*, never an event (PRD-codenames-evals §9
// decision 8), so a record carries no key. The spymaster overlay therefore
// shows the key as the record itself proves it: every identity the game ever
// revealed, tinted on cards that are still face-down at the current step.
// Cards never flipped stay genuinely unknown — see `unknownKeyCards`.

import type {
	CardIdentity,
	Clue,
	GameRecord,
	Seat,
	Team,
} from "../../api/codenames";

/** Key-card composition (crates/engine/src/codenames/types.rs). */
const AGENTS_BY_TEAM: Record<Team, number> = { a: 9, b: 8 };

export interface CardState {
	index: number;
	word: string;
	/** Identity flipped face-up at or before the current step. */
	identity: CardIdentity | null;
	/** Identity the full record proves, for the spymaster overlay. */
	keyIdentity: CardIdentity | null;
	revealedAt: number | null;
	revealedBy: Team | null;
	turn: number | null;
}

export interface GuessEntry {
	idx: number;
	word: string;
	identity: CardIdentity;
	endsTurn: boolean;
	/** True when the card belonged to the guessing team. */
	hit: boolean;
}

export interface TurnBlock {
	turn: number;
	team: Team;
	startIdx: number;
	clue: Clue | null;
	clueIdx: number | null;
	clueSeat: Seat | null;
	guesses: GuessEntry[];
	passedIdx: number | null;
	forced: { idx: number; seat: Seat; decision: string }[];
}

export interface ThoughtEntry {
	seat: Seat;
	decision: string;
	text: string;
	/** The record's own stamp, kept as a stable per-thought identity. */
	atEvent: number;
}

/** Stable identity slug, used for both CSS classes and labels. */
export function identityKey(id: CardIdentity): string {
	return id.kind === "agent" ? `agent-${id.team}` : id.kind;
}

export function identityLabel(id: CardIdentity): string {
	switch (id.kind) {
		case "agent":
			return `Agent ${id.team.toUpperCase()}`;
		case "bystander":
			return "Bystander";
		default:
			return "Assassin";
	}
}

/** The 25 words in grid order, from the setup event. */
function boardWords(record: GameRecord): string[] {
	for (const r of record.events) {
		if (r.event.type === "BoardLaid") return r.event.words;
	}
	return [];
}

/**
 * The grid as it stands after `step` events, with the record-proven key
 * attached to every card the game ever flipped.
 */
export function foldGrid(record: GameRecord, step: number): CardState[] {
	const cards: CardState[] = boardWords(record).map((word, index) => ({
		index,
		word,
		identity: null,
		keyIdentity: null,
		revealedAt: null,
		revealedBy: null,
		turn: null,
	}));
	const byWord = new Map(cards.map((c) => [c.word, c]));
	for (const r of record.events) {
		if (r.event.type !== "GuessRevealed") continue;
		const card = byWord.get(r.event.word);
		if (!card) continue;
		card.keyIdentity = r.event.identity;
		if (r.idx < step) {
			card.identity = r.event.identity;
			card.revealedAt = r.idx;
			card.revealedBy = r.event.team;
			card.turn = r.round;
		}
	}
	return cards;
}

/** Cards the game never flipped, so the overlay cannot know their identity. */
export function unknownKeyCards(cards: CardState[]): number {
	return cards.filter((c) => c.keyIdentity === null).length;
}

/** Agents each team still has to find at the current step. */
export function agentsLeft(cards: CardState[]): Record<Team, number> {
	const left = { ...AGENTS_BY_TEAM };
	for (const c of cards) {
		const id = c.identity;
		if (id?.kind === "agent") left[id.team] -= 1;
	}
	return left;
}

/** Group the transcript into one block per team turn (clue + its guesses). */
export function buildTurns(record: GameRecord): TurnBlock[] {
	const blocks: TurnBlock[] = [];
	const current = () => blocks[blocks.length - 1];
	for (const r of record.events) {
		const e = r.event;
		switch (e.type) {
			case "TurnStarted":
				blocks.push({
					turn: e.turn,
					team: e.team,
					startIdx: r.idx,
					clue: null,
					clueIdx: null,
					clueSeat: null,
					guesses: [],
					passedIdx: null,
					forced: [],
				});
				break;
			case "ClueGiven":
				if (current()) {
					current().clue = e.clue;
					current().clueIdx = r.idx;
					current().clueSeat = e.seat;
				}
				break;
			case "GuessRevealed":
				current()?.guesses.push({
					idx: r.idx,
					word: e.word,
					identity: e.identity,
					endsTurn: e.ends_turn,
					hit: e.identity.kind === "agent" && e.identity.team === e.team,
				});
				break;
			case "TurnPassed":
				if (current()) current().passedIdx = r.idx;
				break;
			case "ForcedDefault":
				current()?.forced.push({
					idx: r.idx,
					seat: e.seat,
					decision: e.decision,
				});
				break;
			default:
				break;
		}
	}
	return blocks;
}

/**
 * Thoughts keyed by the event they explain — the row each one renders directly
 * above.
 *
 * `at_event` is the transcript length *after* the decision's own events landed
 * (every game loop in the repo stamps `state.events.len()` once the action has
 * applied). A guess that ends a turn appends both its `GuessRevealed` and the
 * next `TurnStarted`, so `at_event` points past the following turn's opening;
 * keying on it directly would file that thought under the next turn. Anchor
 * instead on the acting seat's last action event before `at_event`, exactly as
 * `thought_anchors` does for the HTML report.
 */
export function thoughtsByEvent(
	record: GameRecord,
): Map<number, ThoughtEntry[]> {
	const isAction = (type: string) =>
		type === "ClueGiven" || type === "GuessRevealed" || type === "TurnPassed";
	const map = new Map<number, ThoughtEntry[]>();
	const flat = record.seats.flatMap((seat) =>
		seat.thoughts.map((t) => ({ seat: seat.seat, t })),
	);
	// Retries anchor several thoughts to one action: keep the recorded order.
	flat.sort((a, b) => a.t.at_event - b.t.at_event || a.seat - b.seat);
	for (const { seat, t } of flat) {
		const upto = Math.min(t.at_event, record.events.length);
		let anchor = Math.max(0, upto - 1);
		for (let i = upto - 1; i >= 0; i--) {
			const e = record.events[i].event;
			if (isAction(e.type) && "seat" in e && e.seat === seat) {
				anchor = i;
				break;
			}
		}
		const list = map.get(anchor) ?? [];
		list.push({
			seat,
			decision: t.decision,
			text: t.text,
			atEvent: t.at_event,
		});
		map.set(anchor, list);
	}
	return map;
}

/** The card flipped by the step the slider currently sits on, if any. */
export function justRevealed(record: GameRecord, step: number): string | null {
	const r = record.events[step - 1];
	return r && r.event.type === "GuessRevealed" ? r.event.word : null;
}

/**
 * The clue in force at the current step, or `null` when no clue is live.
 *
 * Scanning back stops at the first turn boundary as well as at `GameEnded`: a
 * clue only governs its own turn, so between turns — after a `TurnPassed`, a
 * turn-ending `GuessRevealed`, or the next `TurnStarted`, and before that turn's
 * `ClueGiven` — the banner must show nothing rather than the finished turn's
 * clue. The engine appends `TurnStarted` immediately after a turn-ending guess,
 * so all three boundaries are checked to leave no step in between uncovered.
 */
export function activeClue(
	record: GameRecord,
	step: number,
): { clue: Clue; team: Team; fresh: boolean } | null {
	for (let i = Math.min(step, record.events.length) - 1; i >= 0; i--) {
		const e = record.events[i].event;
		if (e.type === "ClueGiven") {
			return { clue: e.clue, team: e.team, fresh: i === step - 1 };
		}
		if (
			e.type === "GameEnded" ||
			e.type === "TurnPassed" ||
			e.type === "TurnStarted" ||
			(e.type === "GuessRevealed" && e.ends_turn)
		) {
			return null;
		}
	}
	return null;
}
