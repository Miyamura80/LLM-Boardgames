// Shared formatting helpers for the eval viz: human labels, faction colors, and
// a compact one-line rendering of each game event.

import type { GameEvent, Role } from "./types";

export const FACTION_COLOR = {
	liberal: "#3b82f6",
	fascist: "#ef4444",
	hitler: "#111827",
} as const;

export function roleLabel(role: Role): string {
	return role === "hitler"
		? "Hitler"
		: role === "fascist"
			? "Fascist"
			: "Liberal";
}

export function pct(x: number | null | undefined): string {
	return x == null ? "—" : `${(x * 100).toFixed(0)}%`;
}

export function num(x: number | null | undefined, digits = 2): string {
	return x == null ? "—" : x.toFixed(digits);
}

/** A short, public-facing description of an event for the replay timeline. */
export function describeEvent(e: GameEvent): { text: string; tone: string } {
	switch (e.kind) {
		case "game_started":
			return { text: `Game started (${e.seats} players)`, tone: "muted" };
		case "presidency_began":
			return {
				text: `Seat ${e.president} becomes President${e.special_election ? " (special election)" : ""}`,
				tone: "gov",
			};
		case "chancellor_nominated":
			return {
				text: `President ${e.president} nominates seat ${e.nominee}`,
				tone: "gov",
			};
		case "utterance":
			return { text: `Seat ${e.seat}: “${e.text}”`, tone: "talk" };
		case "votes_cast":
			return {
				text: `Vote ${e.passed ? "PASSED" : "failed"} (${e.ja}/${e.needed}): ${e.votes
					.map(([s, v]) => `${s}:${v ? "ja" : "nein"}`)
					.join(" ")}`,
				tone: e.passed ? "pass" : "fail",
			};
		case "government_elected":
			return {
				text: `Government elected: Pres ${e.president}, Chan ${e.chancellor}`,
				tone: "pass",
			};
		case "election_failed":
			return { text: `Election failed (tracker ${e.tracker}/3)`, tone: "fail" };
		case "chaos_policy_enacted":
			return {
				text: `CHAOS: top ${e.policy} policy enacted → ${e.liberal}L / ${e.fascist}F`,
				tone: e.policy === "fascist" ? "fascist" : "liberal",
			};
		case "policy_enacted":
			return {
				text: `${e.policy} policy enacted → ${e.liberal}L / ${e.fascist}F`,
				tone: e.policy === "fascist" ? "fascist" : "liberal",
			};
		case "power_granted":
			return {
				text: `President ${e.president} gains power: ${e.power}`,
				tone: "power",
			};
		case "loyalty_investigated":
			return {
				text: `President ${e.president} investigates seat ${e.target}`,
				tone: "power",
			};
		case "special_election_called":
			return {
				text: `President ${e.president} appoints seat ${e.appointed}`,
				tone: "power",
			};
		case "player_executed":
			return {
				text: `President ${e.president} executes seat ${e.target}`,
				tone: "power",
			};
		case "veto_proposed":
			return {
				text: `Chancellor ${e.chancellor} proposes a veto`,
				tone: "gov",
			};
		case "veto_resolved":
			return {
				text: `President ${e.president} ${e.consented ? "accepts" : "rejects"} the veto`,
				tone: "gov",
			};
		case "game_over":
			return {
				text: `GAME OVER — ${e.winner} win (${e.reason})`,
				tone: "over",
			};
		case "forced_default":
			return {
				text: `Seat ${e.seat} timed out → forced default (${e.decision})`,
				tone: "muted",
			};
		default:
			return { text: e.kind, tone: "muted" };
	}
}
