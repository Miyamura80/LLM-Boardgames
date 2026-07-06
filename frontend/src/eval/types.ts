// TypeScript mirrors of the engine's serialized eval types (see
// `crates/engine/src/eval` and `game`). Only the fields the viz reads are typed;
// events are a discriminated union on `kind`.

export type Role = "liberal" | "fascist" | "hitler";
export type Faction = "liberal" | "fascist";
export type Policy = "liberal" | "fascist";
export type WinReason =
	| "liberal_policies"
	| "fascist_policies"
	| "hitler_executed"
	| "hitler_chancellor";

export interface SeatAssignment {
	seat: number;
	agent: string;
	role: Role;
}

export type GameEvent =
	| { kind: "game_started"; seats: number }
	| { kind: "presidency_began"; president: number; special_election: boolean }
	| { kind: "chancellor_nominated"; president: number; nominee: number }
	| { kind: "utterance"; seat: number; round: number; text: string }
	| {
			kind: "votes_cast";
			votes: [number, boolean][];
			ja: number;
			needed: number;
			passed: boolean;
	  }
	| { kind: "government_elected"; president: number; chancellor: number }
	| { kind: "election_failed"; tracker: number }
	| {
			kind: "chaos_policy_enacted";
			policy: Policy;
			liberal: number;
			fascist: number;
	  }
	| {
			kind: "policy_enacted";
			policy: Policy;
			president: number;
			chancellor: number;
			liberal: number;
			fascist: number;
	  }
	| { kind: "power_granted"; president: number; power: string }
	| { kind: "loyalty_investigated"; president: number; target: number }
	| { kind: "special_election_called"; president: number; appointed: number }
	| { kind: "player_executed"; president: number; target: number }
	| { kind: "veto_proposed"; chancellor: number }
	| { kind: "veto_resolved"; president: number; consented: boolean }
	| { kind: "game_over"; winner: Faction; reason: WinReason }
	| { kind: "forced_default"; seat: number; decision: string }
	| { kind: "role_assigned"; role: Role }
	| { kind: "fascist_team_revealed"; fascists: number[]; hitler: number }
	| { kind: "drew_policies"; policies: Policy[] }
	| { kind: "received_policies"; policies: Policy[] }
	| { kind: "investigation_result"; target: number; party: Faction };

export interface LogEntry {
	event: GameEvent;
	private_to: number | null;
}

export interface BeliefSnapshot {
	checkpoint: number;
	seat: number;
	beliefs: { fascist_prob: Record<string, number> };
}

export interface GameRecord {
	seed: number;
	first_president: number;
	seats: SeatAssignment[];
	winner: Faction;
	win_reason: WinReason;
	log: { entries: LogEntry[] };
	reliability: Record<
		string,
		{
			malformed_outputs: number;
			illegal_moves: number;
			forced_defaults: number;
		}
	>;
	beliefs: BeliefSnapshot[];
	usage: { prompt_tokens: number; completion_tokens: number; calls: number };
}

export interface RoleRating {
	role: Role;
	games: number;
	wins: number;
	mu: number;
	sigma: number;
}

export interface ModelRating {
	model: string;
	roles: RoleRating[];
	total_games: number;
	overall: number;
	liberal_win_rate: number;
	fascist_win_rate: number;
}

export interface ModelRoleMetrics {
	model: string;
	role: Role;
	games: number;
	goal_aligned_enactment: number | null;
	faction_throughput: number | null;
	execution_accuracy: number | null;
	suspicion_brier: number | null;
}

export interface MatchOutcome {
	records: GameRecord[];
	leaderboard: {
		models: ModelRating[];
		total_games: number;
		uncertainty_warning: string | null;
	};
	metrics: ModelRoleMetrics[];
	distribution: { by_model: Record<string, Record<string, number>> };
}
