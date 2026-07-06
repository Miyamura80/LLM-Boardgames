// The role-conditioned Weng-Lin leaderboard: overall μ−2σ plus per-role μ±σ and
// faction win rates, with the objective metric suite alongside.

import { num, pct } from "./format";
import type { MatchOutcome, ModelRoleMetrics, Role } from "./types";

const ROLES: Role[] = ["liberal", "fascist", "hitler"];

function metricFor(
	metrics: ModelRoleMetrics[],
	model: string,
	role: Role,
): ModelRoleMetrics | undefined {
	return metrics.find((m) => m.model === model && m.role === role);
}

export function Leaderboard({ outcome }: { outcome: MatchOutcome }) {
	const { leaderboard, metrics } = outcome;
	return (
		<section className="card">
			<h2>Leaderboard — role-conditioned Weng-Lin (μ − 2σ)</h2>
			{leaderboard.uncertainty_warning && (
				<p className="warn">⚠ {leaderboard.uncertainty_warning}</p>
			)}
			<div className="table-scroll">
				<table>
					<thead>
						<tr>
							<th>Model</th>
							<th>Overall</th>
							<th>Liberal μ±σ</th>
							<th>Fascist μ±σ</th>
							<th>Hitler μ±σ</th>
							<th>Lib WR</th>
							<th>Fasc WR</th>
							<th>Games</th>
						</tr>
					</thead>
					<tbody>
						{leaderboard.models.map((m) => {
							const byRole = Object.fromEntries(
								m.roles.map((r) => [r.role, r]),
							);
							return (
								<tr key={m.model}>
									<td className="mono">{m.model}</td>
									<td className="strong">{num(m.overall, 1)}</td>
									{ROLES.map((role) => {
										const r = byRole[role];
										return (
											<td key={role}>
												{r ? `${num(r.mu, 1)} ± ${num(r.sigma, 1)}` : "—"}
											</td>
										);
									})}
									<td>{pct(m.liberal_win_rate)}</td>
									<td>{pct(m.fascist_win_rate)}</td>
									<td>{m.total_games}</td>
								</tr>
							);
						})}
					</tbody>
				</table>
			</div>

			<h3>Objective metrics (engine-derived, no LLM judge)</h3>
			<div className="table-scroll">
				<table>
					<thead>
						<tr>
							<th>Model</th>
							<th>Role</th>
							<th>Goal-aligned enact</th>
							<th>Faction throughput</th>
							<th>Exec accuracy</th>
							<th>Suspicion Brier ↓</th>
						</tr>
					</thead>
					<tbody>
						{leaderboard.models.flatMap((m) =>
							ROLES.map((role) => {
								const met = metricFor(metrics, m.model, role);
								if (!met || met.games === 0) return null;
								return (
									<tr key={`${m.model}-${role}`}>
										<td className="mono">{m.model}</td>
										<td>{role}</td>
										<td>{pct(met.goal_aligned_enactment)}</td>
										<td>{pct(met.faction_throughput)}</td>
										<td>{pct(met.execution_accuracy)}</td>
										<td>{num(met.suspicion_brier)}</td>
									</tr>
								);
							}),
						)}
					</tbody>
				</table>
			</div>
		</section>
	);
}
