// Game replay: the seat roster (roles revealed post-hoc) and the full public
// event timeline, including discussion utterances — the conversation log.

import { describeEvent, FACTION_COLOR, roleLabel } from "./format";
import type { GameRecord } from "./types";

function SeatRoster({ game }: { game: GameRecord }) {
	return (
		<div className="roster">
			{game.seats.map((s) => (
				<span
					key={s.seat}
					className="seat-chip"
					style={{ borderColor: FACTION_COLOR[s.role] }}
					title={s.agent}
				>
					<b>#{s.seat}</b> {roleLabel(s.role)}
				</span>
			))}
		</div>
	);
}

export function GameReplay({ game }: { game: GameRecord }) {
	// Public timeline only (private events belong to individual seats).
	const publicEvents = game.log.entries.filter((e) => e.private_to === null);
	return (
		<section className="card">
			<div className="replay-head">
				<h3>
					Replay — seed {game.seed}, {game.winner} win ({game.win_reason})
				</h3>
				<span className="muted">
					{game.usage.calls} LLM calls ·{" "}
					{game.usage.prompt_tokens + game.usage.completion_tokens} tokens
				</span>
			</div>
			<SeatRoster game={game} />
			<ol className="timeline">
				{publicEvents.map((entry, i) => {
					const { text, tone } = describeEvent(entry.event);
					return (
						// biome-ignore lint/suspicious/noArrayIndexKey: log is an ordered append-only stream
						<li key={i} className={`ev ev-${tone}`}>
							{text}
						</li>
					);
				})}
			</ol>
		</section>
	);
}
