// Hand-written TS mirrors of the Catan engine types (crates/engine/src/catan)
// plus thin wrappers over the generic command client.

import { callCommand } from "./client";

export type Seat = number;

export interface ResourceSet {
	brick: number;
	lumber: number;
	wool: number;
	grain: number;
	ore: number;
}

export type Terrain =
	| "hills"
	| "forest"
	| "pasture"
	| "fields"
	| "mountains"
	| "desert";

export type Visibility = "Public" | { Private: Seat };

export interface HexSpec {
	hex: number;
	q: number;
	r: number;
	terrain: Terrain;
	number: number | null;
	vertices: [number, number, number, number, number, number];
}

type Port = { kind: "generic" } | { kind: "resource"; resource: string };

export interface PortSpec {
	edge: number;
	vertices: [number, number];
	port: Port;
}

export interface TradeOffer {
	give: ResourceSet;
	receive: ResourceSet;
}

export type TradeResponse =
	| { kind: "accept" }
	| { kind: "reject" }
	| { kind: "counter"; offer: TradeOffer };

// The `type`-tagged event union. Only fields the UI consumes are typed; the
// rest ride along untyped.
export interface CatanEvent {
	type: string;
	seat?: Seat;
	turn?: number;
	vertex?: number;
	edge?: number;
	hex?: number;
	round?: number;
	d1?: number;
	d2?: number;
	free?: boolean;
	card?: string;
	resource?: string;
	text?: string;
	message?: string | null;
	to?: Seat | null;
	offer?: TradeOffer;
	response?: TradeResponse;
	proposer?: Seat;
	with?: Seat;
	winner?: Seat;
	vps?: number[];
	turns?: number;
	length?: number;
	knights?: number;
	previous?: Seat | null;
	gains?: [Seat, ResourceSet][];
	seats?: [Seat, number][];
	gained?: ResourceSet;
	count?: number;
	decision?: string;
	desert?: number;
	hexes?: HexSpec[];
	ports?: PortSpec[];
}

export interface EventRecord {
	idx: number;
	round: number;
	visibility: Visibility;
	event: CatanEvent;
}

interface ThoughtRecord {
	round: number;
	at_event: number;
	decision: string;
	text: string;
}

interface Reliability {
	malformed_outputs: number;
	illegal_moves: number;
	forced_defaults: number;
	transport_failures: number;
}

export interface SeatRecord {
	seat: Seat;
	model_id: string;
	agent_kind: string;
	scaffold_version: string;
	is_anchor: boolean;
	final_vp: number;
	public_vp: number;
	placement: number;
	won: boolean;
	knights_played: number;
	reliability: Reliability;
	thoughts: ThoughtRecord[];
}

export interface GameRecord {
	game_id: string;
	seed: number;
	player_count: number;
	rules_version: string;
	schedule_label: string;
	winner: Seat;
	final_vps: number[];
	turns: number;
	seats: SeatRecord[];
	events: EventRecord[];
	duration_ms: number;
}

export interface SeatLine {
	seat: Seat;
	mu: number;
	sigma: number;
	games: number;
	wins: number;
}

export interface LeaderboardRow {
	model_id: string;
	is_anchor: boolean;
	overall: number;
	games: number;
	wins: number;
	win_rate: number;
	seats: SeatLine[];
}

export interface ModelMetricSummary {
	model_id: string;
	is_anchor: boolean;
	seats: number;
	avg_final_vp: number | null;
	avg_placement: number | null;
	production_actual: number;
	production_expected: number;
	placement_pips_pct: number | null;
	trades_proposed: number;
	trades_executed: number;
	robber_moves: number;
	robber_on_leader: number;
	malformed: number;
	illegal: number;
	forced: number;
	prompt_tokens: number;
	completion_tokens: number;
}

export interface RunSummary {
	run_id: string;
	mode: string;
	status: string;
	games_played: number;
}

export interface GameSummary {
	game_id: string;
	run_id: string;
	schedule_label: string;
	winner: number;
	turns: number;
	duration_ms: number;
}

export function catanListRuns(): Promise<{ runs: RunSummary[] }> {
	return callCommand("catan_list_runs", {});
}

export function catanLeaderboard(runId: string): Promise<{
	run_id: string;
	rows: LeaderboardRow[];
	metrics: ModelMetricSummary[];
	uncertainty_note: string | null;
}> {
	return callCommand("catan_leaderboard", { run_id: runId });
}

export function catanListGames(
	runId: string,
): Promise<{ games: GameSummary[] }> {
	return callCommand("catan_list_games", { run_id: runId });
}

export function catanReplay(
	gameId: string,
): Promise<{ record: GameRecord; rendered: string[] }> {
	return callCommand("catan_game_replay", { game_id: gameId });
}
