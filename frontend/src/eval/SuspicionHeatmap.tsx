// Who-suspected-who: for the final belief checkpoint, a grid of P(target is
// Fascist) as declared privately by each living seat. Ground-truth Fascists are
// marked so mis/well-calibrated suspicion is visible at a glance.

import { roleLabel } from "./format";
import type { GameRecord } from "./types";

function heatColor(p: number): string {
	// Blue (trusts, low P) → red (suspects, high P).
	const r = Math.round(59 + (239 - 59) * p);
	const g = Math.round(130 + (68 - 130) * p);
	const b = Math.round(246 + (68 - 246) * p);
	return `rgb(${r}, ${g}, ${b})`;
}

export function SuspicionHeatmap({ game }: { game: GameRecord }) {
	if (game.beliefs.length === 0) {
		return (
			<section className="card">
				<h3>Suspicion heatmap</h3>
				<p className="muted">
					No beliefs elicited for this game (run without --no-beliefs).
				</p>
			</section>
		);
	}

	const lastCheckpoint = Math.max(...game.beliefs.map((b) => b.checkpoint));
	const snaps = game.beliefs.filter((b) => b.checkpoint === lastCheckpoint);
	const seats = game.seats.map((s) => s.seat);
	const bySeat = new Map(snaps.map((s) => [s.seat, s.beliefs.fascist_prob]));
	const isFascist = (seat: number) => game.seats[seat].role !== "liberal";

	return (
		<section className="card">
			<h3>Suspicion heatmap — final P(target is Fascist), by observer</h3>
			<p className="muted">
				Rows = observer, columns = target. Red = suspects Fascist, blue =
				trusts. ★ marks true Fascists/Hitler.
			</p>
			<div className="table-scroll">
				<table className="heatmap">
					<thead>
						<tr>
							<th>obs \ tgt</th>
							{seats.map((t) => (
								<th key={t}>
									{t}
									{isFascist(t) ? " ★" : ""}
								</th>
							))}
						</tr>
					</thead>
					<tbody>
						{seats.map((observer) => {
							const probs = bySeat.get(observer);
							return (
								<tr key={observer}>
									<th>
										{observer} ({roleLabel(game.seats[observer].role)[0]})
									</th>
									{seats.map((target) => {
										if (observer === target)
											return <td key={target} className="diag" />;
										const p = probs?.[String(target)];
										return (
											<td
												key={target}
												style={
													p == null
														? undefined
														: { background: heatColor(p), color: "#fff" }
												}
											>
												{p == null ? "—" : p.toFixed(2)}
											</td>
										);
									})}
								</tr>
							);
						})}
					</tbody>
				</table>
			</div>
		</section>
	);
}
