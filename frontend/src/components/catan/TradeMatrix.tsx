// Who traded what with whom: directional card flow across executed player
// trades — the Catan analogue of the SH suspicion heatmap.

import type { EventRecord } from "../../api/catan";
import { PLAYER_COLORS } from "./board";

export function TradeMatrix({ events }: { events: EventRecord[] }) {
	// flow[a][b] = cards a handed to b via executed trades.
	const flow: number[][] = Array.from({ length: 4 }, () => [0, 0, 0, 0]);
	let executed = 0;
	for (const { event: e } of events) {
		if (e.type !== "TradeExecuted" || e.proposer === undefined) continue;
		const partner = e.with;
		if (partner === undefined || !e.offer) continue;
		const give = Object.values(e.offer.give).reduce((s, n) => s + n, 0);
		const receive = Object.values(e.offer.receive).reduce((s, n) => s + n, 0);
		flow[e.proposer][partner] += give;
		flow[partner][e.proposer] += receive;
		executed += 1;
	}
	const max = Math.max(1, ...flow.flat());

	return (
		<div className="catan-panel">
			<h3>Trade flow</h3>
			<p className="catan-dim">
				{executed === 0
					? "No player trades executed."
					: `${executed} executed trade(s) — cards row → column.`}
			</p>
			<table className="catan-matrix">
				<thead>
					<tr>
						<th aria-label="giver" />
						{[0, 1, 2, 3].map((s) => (
							<th key={s} style={{ color: PLAYER_COLORS[s] }}>
								P{s}
							</th>
						))}
					</tr>
				</thead>
				<tbody>
					{[0, 1, 2, 3].map((from) => (
						<tr key={from}>
							<th style={{ color: PLAYER_COLORS[from] }}>P{from}</th>
							{[0, 1, 2, 3].map((to) => (
								<td
									key={to}
									style={{
										background:
											from === to
												? "transparent"
												: `rgba(64, 46, 24, ${(flow[from][to] / max) * 0.75})`,
									}}
								>
									{from === to ? "—" : flow[from][to] || ""}
								</td>
							))}
						</tr>
					))}
				</tbody>
			</table>
		</div>
	);
}
