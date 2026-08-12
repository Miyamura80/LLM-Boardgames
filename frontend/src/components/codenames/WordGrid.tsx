// The 5×5 word grid. Cards start face-up as words; a reveal flips the card to
// its identity's color. With the spymaster overlay on, still-face-down cards
// carry a tinted edge and a small key pip, so the viewer can read the board the
// way the spymaster did. Color is never the only signal — every identity also
// carries a letter tag (accessibility, and the assassin must be unmistakable).

import type { CardIdentity } from "../../api/codenames";
import { type CardState, identityKey, identityLabel } from "./replay";

function tag(id: CardIdentity): string {
	switch (id.kind) {
		case "agent":
			return id.team.toUpperCase();
		case "bystander":
			return "•";
		default:
			return "☠";
	}
}

interface Props {
	cards: CardState[];
	/** Omniscient view: tint face-down cards with the record-proven key. */
	spymaster: boolean;
	/** Word flipped by the event the slider sits on, pulsed for one step. */
	activeWord: string | null;
	/** Shrink the cards so the grid can sit inside a turn block as a snapshot. */
	compact?: boolean;
}

export function WordGrid({ cards, spymaster, activeWord, compact }: Props) {
	return (
		<div className={compact ? "cn-grid cn-grid-mini" : "cn-grid"}>
			{cards.map((card) => (
				<Card
					card={card}
					key={card.word}
					spymaster={spymaster}
					active={card.word === activeWord}
				/>
			))}
		</div>
	);
}

function Card({
	card,
	spymaster,
	active,
}: {
	card: CardState;
	spymaster: boolean;
	active: boolean;
}) {
	const face = card.identity;
	const hint = spymaster && !face ? card.keyIdentity : null;
	const classes = ["cn-card"];
	if (face) classes.push("cn-card-up", `cn-id-${identityKey(face)}`);
	else classes.push("cn-card-down");
	if (hint) classes.push("cn-key", `cn-key-${identityKey(hint)}`);
	if (spymaster && !face && !card.keyIdentity) classes.push("cn-key-unknown");
	if (active) classes.push("cn-card-active");

	const state = face
		? `revealed ${identityLabel(face)} on turn ${card.turn}`
		: hint
			? `face down, key says ${identityLabel(hint)}`
			: "face down";

	return (
		<div className="cn-card-slot">
			<div className={classes.join(" ")}>
				<span className="cn-card-word">{card.word}</span>
				<span className="cn-sr">{state}</span>
				{face && (
					<span className="cn-card-tag">
						{tag(face)}
						<span className="cn-card-turn">t{card.turn}</span>
					</span>
				)}
				{hint && <span className="cn-card-pip">{tag(hint)}</span>}
				{spymaster && !face && !card.keyIdentity && (
					<span className="cn-card-pip">?</span>
				)}
			</div>
		</div>
	);
}
