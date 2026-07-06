// Who-suspected-who heatmap: rows = believers, columns = subjects, cell value
// = the believer's stated P(subject is on the Fascist team) at a checkpoint.
//
// Color job: MAGNITUDE → one sequential hue, light→dark (never a rainbow).
// Ground-truth identity is carried by a marker + legend, never by cell color.

import type { BeliefSnapshot, Role } from "../api/sh";

// Single-hue sequential ramp (parchment → deep fascist red): the cell measures
// P(subject is on the Fascist team), so magnitude climbs toward the brand red.
// Text flips to parchment for the dark steps. Ground-truth identity is still
// carried by the ✦ marker + legend, never by cell color alone.
const RAMP = [
	"#f6eeda",
	"#f2ddc0",
	"#eec39c",
	"#e7a077",
	"#df7d55",
	"#d65e3b",
	"#c33a22",
	"#9f2c19",
	"#6f1d10",
];

function cellColor(v: number): { bg: string; dark: boolean } {
	const idx = Math.max(
		0,
		Math.min(RAMP.length - 1, Math.floor(v * RAMP.length)),
	);
	return { bg: RAMP[idx], dark: idx >= 5 };
}

interface HeatmapProps {
	beliefs: BeliefSnapshot[];
	checkpoint: number;
	roles: Role[];
	alive: boolean[];
}

export function Heatmap({ beliefs, checkpoint, roles, alive }: HeatmapProps) {
	const snaps = beliefs.filter((b) => b.checkpoint === checkpoint);
	if (snaps.length === 0) {
		return <p className="sh-muted">No belief snapshot at this checkpoint.</p>;
	}
	const seats = roles.map((_, i) => i);

	return (
		<div className="sh-table-wrap">
			<table className="sh-heatmap" aria-label="who suspects whom">
				<thead>
					<tr>
						<th scope="col">believer ↓ / subject →</th>
						{seats.map((s) => (
							<th key={s} scope="col">
								P{s}
								{roles[s] !== "Liberal" && (
									<span title={`ground truth: ${roles[s]}`}> ✦</span>
								)}
							</th>
						))}
					</tr>
				</thead>
				<tbody>
					{seats.map((believer) => {
						const snap = snaps.find((b) => b.seat === believer);
						return (
							<tr key={believer}>
								<th scope="row">
									P{believer}
									<span className="sh-muted sh-small"> {roles[believer]}</span>
									{!alive[believer] && <span className="sh-muted"> †</span>}
								</th>
								{seats.map((subject) => {
									const probs = snap?.report.assessments[String(subject)];
									if (believer === subject || !probs) {
										return <td key={subject} className="sh-hm-empty" />;
									}
									const p = probs.fascist + probs.hitler;
									const { bg, dark } = cellColor(p);
									return (
										<td
											key={subject}
											className="sh-hm-cell"
											style={{
												background: bg,
												color: dark ? "#fff" : "#171717",
											}}
											title={`P${believer} believes P${subject}: Liberal ${(probs.liberal * 100).toFixed(0)}%, Fascist ${(probs.fascist * 100).toFixed(0)}%, Hitler ${(probs.hitler * 100).toFixed(0)}%`}
										>
											{(p * 100).toFixed(0)}
										</td>
									);
								})}
							</tr>
						);
					})}
				</tbody>
			</table>
			<p className="sh-muted sh-small">
				Cell = stated P(subject is on the Fascist team), 0–100. ✦ marks the true
				Fascists/Hitler; † marks executed players. Beliefs are private — no
				player saw another's row.
			</p>
		</div>
	);
}
