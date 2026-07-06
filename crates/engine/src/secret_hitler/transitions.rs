//! State transitions: applying validated agent actions to the game state.
//!
//! `apply` is the single mutation entry point. It validates legality (returning
//! [`IllegalMove`] with a reason suitable for a rethink re-prompt) and advances
//! the phase machine, emitting transcript events along the way.

use super::actions::{Action, IllegalMove};
use super::events::{GameEvent, Visibility};
use super::state::{GameState, Phase};
use super::types::*;

impl GameState {
    /// Apply `action` for `seat`. Returns `Err(IllegalMove)` without mutating
    /// state when the move is not legal for the current phase.
    pub fn apply(&mut self, seat: Seat, action: Action) -> Result<(), IllegalMove> {
        if self.is_over() {
            return Err(IllegalMove("the game is already over".into()));
        }
        if !self.is_alive(seat) {
            return Err(IllegalMove(format!("Player {seat} is dead and cannot act")));
        }
        match (self.phase.clone(), action) {
            (Phase::Nomination, Action::Nominate { target }) => self.nominate(seat, target),
            (Phase::Election { nominee, .. }, Action::Vote { ja }) => self.vote(seat, nominee, ja),
            (Phase::LegislativePresident { tiles }, Action::Discard { policy }) => {
                self.president_discard(seat, &tiles, policy)
            }
            (
                Phase::LegislativeChancellor {
                    tiles,
                    veto_proposed,
                },
                Action::Enact { policy },
            ) => self.chancellor_enact(seat, &tiles, veto_proposed, policy),
            (
                Phase::LegislativeChancellor {
                    tiles,
                    veto_proposed,
                },
                Action::ProposeVeto,
            ) => self.propose_veto(seat, tiles, veto_proposed),
            (Phase::VetoConsent { tiles }, Action::VetoConsent { approve }) => {
                self.veto_consent(seat, tiles, approve)
            }
            (Phase::ExecutiveAction { power }, Action::UsePower { target }) => {
                self.use_power(seat, power, target)
            }
            (phase, action) => Err(IllegalMove(format!(
                "action {action:?} is not valid in the current phase ({})",
                phase_name(&phase)
            ))),
        }
    }

    fn nominate(&mut self, seat: Seat, target: Seat) -> Result<(), IllegalMove> {
        if seat != self.presidency {
            return Err(IllegalMove(format!(
                "only President Player {} may nominate",
                self.presidency
            )));
        }
        let eligible = self.eligible_chancellors();
        if !eligible.contains(&target) {
            return Err(IllegalMove(format!(
                "Player {target} is not an eligible Chancellor (eligible: {eligible:?}); dead, term-limited, and the President are excluded"
            )));
        }
        self.push_event(
            Visibility::Public,
            GameEvent::ChancellorNominated {
                president: seat,
                nominee: target,
            },
        );
        self.phase = Phase::Election {
            nominee: target,
            votes: Default::default(),
        };
        Ok(())
    }

    fn vote(&mut self, seat: Seat, nominee: Seat, ja: bool) -> Result<(), IllegalMove> {
        let Phase::Election { votes, .. } = &mut self.phase else {
            unreachable!("vote() called outside Election");
        };
        if votes.contains_key(&seat) {
            return Err(IllegalMove(format!("Player {seat} has already voted")));
        }
        votes.insert(seat, ja);

        let alive = self.alive_seats();
        let Phase::Election { votes, .. } = &self.phase else {
            unreachable!();
        };
        if votes.len() < alive.len() {
            return Ok(()); // ballots stay hidden until everyone has voted
        }

        // All ballots in: reveal simultaneously and resolve.
        let votes: Vec<(Seat, bool)> = votes.iter().map(|(s, v)| (*s, *v)).collect();
        let ja_count = votes.iter().filter(|(_, v)| *v).count();
        let passed = ja_count * 2 > alive.len();
        self.push_event(
            Visibility::Public,
            GameEvent::VotesRevealed {
                nominee,
                votes,
                passed,
            },
        );

        if passed {
            self.election_tracker = 0;
            self.gov_chancellor = Some(nominee);
            self.term_limited_president = Some(self.presidency);
            self.term_limited_chancellor = Some(nominee);
            self.push_event(
                Visibility::Public,
                GameEvent::GovernmentFormed {
                    president: self.presidency,
                    chancellor: nominee,
                },
            );
            // Fascists win the moment Hitler is elected Chancellor after the
            // 3rd Fascist policy.
            if self.fascist_policies >= HITLER_CHANCELLOR_THRESHOLD
                && self.players[nominee as usize].role == Role::Hitler
            {
                self.end_game(WinCondition::HitlerChancellor);
                return Ok(());
            }
            let tiles = self.draw_tiles(3);
            self.push_event(
                Visibility::Private(self.presidency),
                GameEvent::PresidentDrew {
                    tiles: tiles.clone(),
                },
            );
            self.phase = Phase::LegislativePresident { tiles };
        } else {
            self.advance_tracker();
        }
        Ok(())
    }

    fn president_discard(
        &mut self,
        seat: Seat,
        tiles: &[Party],
        policy: Party,
    ) -> Result<(), IllegalMove> {
        if seat != self.presidency {
            return Err(IllegalMove("only the President may discard".into()));
        }
        let remaining = remove_tile(tiles, policy).ok_or_else(|| {
            IllegalMove(format!(
                "you do not hold a {} policy to discard (your tiles: {tiles:?})",
                policy.as_str()
            ))
        })?;
        self.deck.discard_tile(policy);
        self.push_event(
            Visibility::Private(seat),
            GameEvent::PresidentDiscarded { policy },
        );
        let chancellor = self.gov_chancellor.expect("government formed");
        self.push_event(
            Visibility::Private(chancellor),
            GameEvent::ChancellorReceived {
                tiles: remaining.clone(),
            },
        );
        self.phase = Phase::LegislativeChancellor {
            tiles: remaining,
            veto_proposed: false,
        };
        Ok(())
    }

    fn chancellor_enact(
        &mut self,
        seat: Seat,
        tiles: &[Party],
        veto_proposed: bool,
        policy: Party,
    ) -> Result<(), IllegalMove> {
        self.ensure_chancellor(seat)?;
        let rest = remove_tile(tiles, policy).ok_or_else(|| {
            IllegalMove(format!(
                "you do not hold a {} policy to enact (your tiles: {tiles:?})",
                policy.as_str()
            ))
        })?;
        // keep the phase's veto flag meaningful if we ever return here
        let _ = veto_proposed;
        for t in rest {
            self.deck.discard_tile(t);
        }
        self.enact_policy(policy, true);
        Ok(())
    }

    fn propose_veto(
        &mut self,
        seat: Seat,
        tiles: Vec<Party>,
        veto_proposed: bool,
    ) -> Result<(), IllegalMove> {
        self.ensure_chancellor(seat)?;
        if !self.veto_unlocked() {
            return Err(IllegalMove(
                "veto power is not unlocked until 5 Fascist policies are enacted".into(),
            ));
        }
        if veto_proposed {
            return Err(IllegalMove(
                "the veto was already declined this session; you must enact a policy".into(),
            ));
        }
        self.push_event(
            Visibility::Public,
            GameEvent::VetoProposed { chancellor: seat },
        );
        self.phase = Phase::VetoConsent { tiles };
        Ok(())
    }

    fn veto_consent(
        &mut self,
        seat: Seat,
        tiles: Vec<Party>,
        approve: bool,
    ) -> Result<(), IllegalMove> {
        if seat != self.presidency {
            return Err(IllegalMove("only the President decides on a veto".into()));
        }
        self.push_event(
            Visibility::Public,
            GameEvent::VetoDecided {
                president: seat,
                approved: approve,
            },
        );
        if approve {
            for t in tiles {
                self.deck.discard_tile(t);
            }
            // A vetoed agenda counts as an inactive government: tracker +1.
            self.advance_tracker();
        } else {
            self.phase = Phase::LegislativeChancellor {
                tiles,
                veto_proposed: true,
            };
        }
        Ok(())
    }

    fn use_power(&mut self, seat: Seat, power: Power, target: Seat) -> Result<(), IllegalMove> {
        if seat != self.presidency {
            return Err(IllegalMove(
                "only the President uses executive powers".into(),
            ));
        }
        let targets = self.power_targets(power);
        if !targets.contains(&target) {
            return Err(IllegalMove(format!(
                "Player {target} is not a legal target for {power:?} (legal: {targets:?})"
            )));
        }
        match power {
            Power::InvestigateLoyalty => {
                self.investigated.push(target);
                self.push_event(
                    Visibility::Public,
                    GameEvent::Investigated {
                        president: seat,
                        target,
                    },
                );
                let party = self.players[target as usize].role.party();
                self.push_event(
                    Visibility::Private(seat),
                    GameEvent::InvestigationResult { target, party },
                );
                self.advance_presidency();
            }
            Power::SpecialElection => {
                self.push_event(
                    Visibility::Public,
                    GameEvent::SpecialElectionCalled {
                        president: seat,
                        target,
                    },
                );
                // Normal order resumes after the special round, from the seat
                // left of the President who called it.
                self.special_return = Some(self.next_alive_after(seat));
                self.gov_chancellor = None;
                self.round += 1;
                self.presidency = target;
                self.phase = Phase::Nomination;
            }
            Power::Execution => {
                let was_hitler = self.players[target as usize].role == Role::Hitler;
                self.players[target as usize].alive = false;
                self.push_event(
                    Visibility::Public,
                    GameEvent::Executed {
                        president: seat,
                        target,
                        was_hitler,
                    },
                );
                if was_hitler {
                    self.end_game(WinCondition::HitlerExecuted);
                } else {
                    self.advance_presidency();
                }
            }
        }
        Ok(())
    }

    // -- shared helpers -------------------------------------------------------

    fn ensure_chancellor(&self, seat: Seat) -> Result<(), IllegalMove> {
        if Some(seat) != self.gov_chancellor {
            return Err(IllegalMove("only the Chancellor may do this".into()));
        }
        Ok(())
    }

    /// Draw tiles, emitting a public reshuffle event when the discard pile was
    /// folded back in.
    fn draw_tiles(&mut self, n: usize) -> Vec<Party> {
        let before = self.deck.reshuffles;
        let tiles = self.deck.draw(n, &mut self.rng);
        if self.deck.reshuffles > before {
            let draw_count = (self.deck.draw_count() + tiles.len()) as u8;
            self.push_event(Visibility::Public, GameEvent::DeckReshuffled { draw_count });
        }
        tiles
    }

    /// Enact a policy tile. `by_government` policies fire board powers;
    /// top-decked ones never do.
    fn enact_policy(&mut self, policy: Party, by_government: bool) {
        match policy {
            Party::Liberal => self.liberal_policies += 1,
            Party::Fascist => self.fascist_policies += 1,
        }
        self.push_event(Visibility::Public, GameEvent::PolicyEnacted { policy });

        if self.liberal_policies >= LIBERAL_WIN_POLICIES {
            self.end_game(WinCondition::LiberalPolicies);
            return;
        }
        if self.fascist_policies >= FASCIST_WIN_POLICIES {
            self.end_game(WinCondition::FascistPolicies);
            return;
        }

        if by_government && policy == Party::Fascist {
            if let Some(power) = power_for_fascist_policy(self.fascist_policies) {
                self.push_event(
                    Visibility::Public,
                    GameEvent::PowerGranted {
                        president: self.presidency,
                        power,
                    },
                );
                self.phase = Phase::ExecutiveAction { power };
                return;
            }
        }
        self.advance_presidency();
    }

    /// Advance the election tracker and move to the next round. At 3 failed
    /// governments the top policy is auto-enacted with no power, the tracker
    /// resets, and term-limit memory clears. Owns the whole continuation:
    /// callers must not additionally advance the presidency.
    fn advance_tracker(&mut self) {
        self.election_tracker += 1;
        self.push_event(
            Visibility::Public,
            GameEvent::ElectionTrackerAdvanced {
                value: self.election_tracker,
            },
        );
        if self.election_tracker >= ELECTION_TRACKER_LIMIT {
            let tile = self.draw_tiles(1)[0];
            self.election_tracker = 0;
            self.term_limited_president = None;
            self.term_limited_chancellor = None;
            self.push_event(
                Visibility::Public,
                GameEvent::TopDeckEnacted { policy: tile },
            );
            // enact_policy advances the presidency (or ends the game) itself.
            self.enact_policy(tile, false);
        } else {
            self.advance_presidency();
        }
    }
}

fn phase_name(phase: &Phase) -> &'static str {
    match phase {
        Phase::Nomination => "nomination",
        Phase::Election { .. } => "election",
        Phase::LegislativePresident { .. } => "legislative-president",
        Phase::LegislativeChancellor { .. } => "legislative-chancellor",
        Phase::VetoConsent { .. } => "veto-consent",
        Phase::ExecutiveAction { .. } => "executive-action",
        Phase::GameOver => "game-over",
    }
}

/// Remove one tile of `policy` from `tiles`, returning the rest, or `None` if
/// no such tile is held.
fn remove_tile(tiles: &[Party], policy: Party) -> Option<Vec<Party>> {
    let pos = tiles.iter().position(|&t| t == policy)?;
    let mut rest = tiles.to_vec();
    rest.remove(pos);
    Some(rest)
}
