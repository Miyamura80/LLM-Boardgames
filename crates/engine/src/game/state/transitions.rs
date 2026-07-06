//! State-machine transitions: applying an [`Action`], advancing phases, firing
//! presidential powers, and checking win conditions. Split from `state/mod.rs`
//! to keep files under the repo's line budget; as a child module it retains
//! access to `GameState`'s private fields.

use super::{GameState, Phase};
use crate::game::action::{Action, IllegalAction};
use crate::game::board::{power_for_fascist_count, Power};
use crate::game::log::{Event, WinReason};
use crate::game::policy::Policy;
use crate::game::roles::{Faction, Role};

impl GameState {
    /// Validate and apply an action. Returns the events emitted by this
    /// transition, or a typed [`IllegalAction`] (the state is unchanged on Err).
    pub fn apply(&mut self, action: Action) -> Result<Vec<Event>, IllegalAction> {
        if self.is_over() {
            return Err(IllegalAction::GameOver);
        }
        let before = self.log.entries.len();
        self.dispatch(action)?;
        Ok(self
            .log
            .entries
            .iter()
            .skip(before)
            .map(|e| e.event.clone())
            .collect())
    }

    fn dispatch(&mut self, action: Action) -> Result<(), IllegalAction> {
        match self.phase.clone() {
            Phase::Nomination => self.act_nominate(action),
            Phase::AwaitingVotes { nominee } => self.act_vote(action, nominee),
            Phase::PresidentLegislative { drawn } => self.act_president_discard(action, drawn),
            Phase::ChancellorLegislative {
                hand,
                veto_available,
            } => self.act_chancellor(action, hand, veto_available),
            Phase::AwaitingVetoConsent { hand } => self.act_veto_consent(action, hand),
            Phase::PresidentialPower { power } => self.act_power(action, power),
            Phase::GameOver => Err(IllegalAction::GameOver),
        }
    }

    fn act_nominate(&mut self, action: Action) -> Result<(), IllegalAction> {
        let Action::Nominate(nominee) = action else {
            return Err(IllegalAction::WrongActionType);
        };
        if !self.eligible_chancellors().contains(&nominee) {
            return Err(IllegalAction::IllegalTarget(nominee));
        }
        self.log.public(Event::ChancellorNominated {
            president: self.president,
            nominee,
        });
        self.phase = Phase::AwaitingVotes { nominee };
        Ok(())
    }

    fn act_vote(&mut self, action: Action, nominee: usize) -> Result<(), IllegalAction> {
        let Action::CastVotes(votes) = action else {
            return Err(IllegalAction::WrongActionType);
        };
        // Ballot must cover exactly the living voters, once each.
        let living = self.living_seats();
        if votes.len() != living.len() || !living.iter().all(|s| votes.contains_key(s)) {
            return Err(IllegalAction::MalformedBallot);
        }
        let ja = votes.values().filter(|&&v| v).count();
        let needed = living.len() / 2 + 1;
        let passed = ja >= needed;
        self.log.public(Event::VotesCast {
            votes: votes.iter().map(|(&s, &v)| (s, v)).collect(),
            ja,
            needed,
            passed,
        });

        if passed {
            self.on_government_elected(nominee)
        } else {
            self.on_election_failed()
        }
    }

    fn on_government_elected(&mut self, nominee: usize) -> Result<(), IllegalAction> {
        self.log.public(Event::GovernmentElected {
            president: self.president,
            chancellor: nominee,
        });
        self.chancellor = Some(nominee);

        // Hitler-Chancellor win fires on election, before any policy phase.
        if self.board.hitler_chancellor_wins() && self.players[nominee].role == Role::Hitler {
            self.declare_winner(Faction::Fascist, WinReason::HitlerChancellor);
            return Ok(());
        }

        // A successful election resets the tracker and sets term limits.
        self.election_tracker = 0;
        self.last_president = Some(self.president);
        self.last_chancellor = Some(nominee);

        // President draws three (reshuffle first if needed).
        self.deck.maybe_reshuffle(&mut self.rng);
        let drawn = self.deck.draw_three();
        self.log.private(
            self.president,
            Event::DrewPolicies {
                policies: drawn.to_vec(),
            },
        );
        self.phase = Phase::PresidentLegislative { drawn };
        Ok(())
    }

    fn on_election_failed(&mut self) -> Result<(), IllegalAction> {
        self.chancellor = None;
        self.election_tracker += 1;
        self.log.public(Event::ElectionFailed {
            tracker: self.election_tracker,
        });
        if self.election_tracker >= 3 {
            self.chaos_topdeck();
            if self.is_over() {
                return Ok(());
            }
        }
        self.begin_next_presidency();
        Ok(())
    }

    /// Election tracker hit 3: top policy auto-enacts, grants no power, tracker
    /// resets, term-limit memory clears.
    fn chaos_topdeck(&mut self) {
        self.deck.maybe_reshuffle(&mut self.rng);
        let policy = self.deck.take_top().expect("deck non-empty for top-deck");
        self.apply_policy_to_board(policy);
        self.log.public(Event::ChaosPolicyEnacted {
            policy,
            liberal: self.board.liberal,
            fascist: self.board.fascist,
        });
        self.election_tracker = 0;
        self.last_president = None;
        self.last_chancellor = None;
        self.check_policy_win(policy);
    }

    fn act_president_discard(
        &mut self,
        action: Action,
        drawn: [Policy; 3],
    ) -> Result<(), IllegalAction> {
        let Action::Discard(i) = action else {
            return Err(IllegalAction::WrongActionType);
        };
        if i >= drawn.len() {
            return Err(IllegalAction::IndexOutOfRange(i));
        }
        self.deck.discard([drawn[i]]);
        let hand: [Policy; 2] = {
            let kept: Vec<Policy> = drawn
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, &p)| p)
                .collect();
            [kept[0], kept[1]]
        };
        let chancellor = self.chancellor.expect("chancellor set");
        self.log.private(
            chancellor,
            Event::ReceivedPolicies {
                policies: hand.to_vec(),
            },
        );
        self.phase = Phase::ChancellorLegislative {
            hand,
            veto_available: self.board.veto_unlocked(),
        };
        Ok(())
    }

    fn act_chancellor(
        &mut self,
        action: Action,
        hand: [Policy; 2],
        veto_available: bool,
    ) -> Result<(), IllegalAction> {
        match action {
            Action::Enact(i) => {
                if i >= hand.len() {
                    return Err(IllegalAction::IndexOutOfRange(i));
                }
                let enacted = hand[i];
                let discarded = hand[1 - i];
                self.deck.discard([discarded]);
                self.enact_government_policy(enacted);
                Ok(())
            }
            Action::ProposeVeto => {
                if !veto_available {
                    return Err(IllegalAction::VetoUnavailable);
                }
                let chancellor = self.chancellor.expect("chancellor set");
                self.log.public(Event::VetoProposed { chancellor });
                self.phase = Phase::AwaitingVetoConsent { hand };
                Ok(())
            }
            _ => Err(IllegalAction::WrongActionType),
        }
    }

    fn act_veto_consent(&mut self, action: Action, hand: [Policy; 2]) -> Result<(), IllegalAction> {
        let Action::VetoConsent(consent) = action else {
            return Err(IllegalAction::WrongActionType);
        };
        self.log.public(Event::VetoResolved {
            president: self.president,
            consented: consent,
        });
        if consent {
            // Both tiles discarded, no policy, tracker advances like a failure.
            self.deck.discard(hand);
            self.election_tracker += 1;
            self.log.public(Event::ElectionFailed {
                tracker: self.election_tracker,
            });
            if self.election_tracker >= 3 {
                self.chaos_topdeck();
                if self.is_over() {
                    return Ok(());
                }
            }
            self.begin_next_presidency();
        } else {
            // President refused: Chancellor must now enact (veto spent).
            self.phase = Phase::ChancellorLegislative {
                hand,
                veto_available: false,
            };
        }
        Ok(())
    }

    fn act_power(&mut self, action: Action, power: Power) -> Result<(), IllegalAction> {
        let targets = self.power_targets(power);
        match power {
            Power::InvestigateLoyalty => {
                let Action::Investigate(t) = action else {
                    return Err(IllegalAction::WrongActionType);
                };
                if !targets.contains(&t) {
                    return Err(IllegalAction::IllegalTarget(t));
                }
                let party = self.players[t].role.party();
                self.investigated.push(t);
                self.investigation_results
                    .entry(self.president)
                    .or_default()
                    .push((t, party));
                self.log.public(Event::LoyaltyInvestigated {
                    president: self.president,
                    target: t,
                });
                self.log.private(
                    self.president,
                    Event::InvestigationResult { target: t, party },
                );
                self.begin_next_presidency();
                Ok(())
            }
            Power::SpecialElection => {
                let Action::SpecialElection(t) = action else {
                    return Err(IllegalAction::WrongActionType);
                };
                if !targets.contains(&t) {
                    return Err(IllegalAction::IllegalTarget(t));
                }
                self.pending_special = Some(t);
                self.special_return = Some(self.next_living_after(self.president));
                self.log.public(Event::SpecialElectionCalled {
                    president: self.president,
                    appointed: t,
                });
                self.begin_next_presidency();
                Ok(())
            }
            Power::Execution => {
                let Action::Execute(t) = action else {
                    return Err(IllegalAction::WrongActionType);
                };
                if !targets.contains(&t) {
                    return Err(IllegalAction::IllegalTarget(t));
                }
                self.players[t].alive = false;
                self.log.public(Event::PlayerExecuted {
                    president: self.president,
                    target: t,
                });
                if self.players[t].role == Role::Hitler {
                    self.declare_winner(Faction::Liberal, WinReason::HitlerExecuted);
                    return Ok(());
                }
                self.begin_next_presidency();
                Ok(())
            }
        }
    }

    // -----------------------------------------------------------------------
    // Policy enactment + powers
    // -----------------------------------------------------------------------

    fn apply_policy_to_board(&mut self, policy: Policy) {
        match policy {
            Policy::Liberal => self.board.liberal += 1,
            Policy::Fascist => self.board.fascist += 1,
        }
    }

    /// Enact a policy chosen by an elected government, firing a power if the
    /// Fascist count landed on a power square.
    fn enact_government_policy(&mut self, policy: Policy) {
        self.apply_policy_to_board(policy);
        let (president, chancellor) = (self.president, self.chancellor.expect("chancellor set"));
        self.log.public(Event::PolicyEnacted {
            policy,
            president,
            chancellor,
            liberal: self.board.liberal,
            fascist: self.board.fascist,
        });

        self.check_policy_win(policy);
        if self.is_over() {
            return;
        }

        if policy == Policy::Fascist {
            if let Some(power) = power_for_fascist_count(self.board.fascist) {
                // Only fire if there is a legal target; otherwise skip.
                if self.power_targets(power).is_empty() {
                    self.begin_next_presidency();
                } else {
                    self.log.public(Event::PowerGranted { president, power });
                    self.phase = Phase::PresidentialPower { power };
                }
                return;
            }
        }
        self.begin_next_presidency();
    }

    fn check_policy_win(&mut self, _policy: Policy) {
        if self.board.liberals_complete() {
            self.declare_winner(Faction::Liberal, WinReason::LiberalPolicies);
        } else if self.board.fascists_complete() {
            self.declare_winner(Faction::Fascist, WinReason::FascistPolicies);
        }
    }

    fn declare_winner(&mut self, faction: Faction, reason: WinReason) {
        self.winner = Some((faction, reason));
        self.phase = Phase::GameOver;
        self.log.public(Event::GameOver {
            winner: faction,
            reason,
        });
    }

    /// Advance to the next presidency (honoring a pending special election) and
    /// open nominations.
    fn begin_next_presidency(&mut self) {
        if self.is_over() {
            return;
        }
        self.chancellor = None;

        let special = if let Some(a) = self.pending_special.take() {
            self.president = a;
            self.current_is_special = true;
            true
        } else if self.current_is_special {
            self.current_is_special = false;
            let r = self
                .special_return
                .take()
                .map(|r| self.first_living_at_or_after(r))
                .unwrap_or_else(|| self.next_living_after(self.president));
            self.president = r;
            false
        } else {
            self.president = self.next_living_after(self.president);
            false
        };

        self.phase = Phase::Nomination;
        self.log.public(Event::PresidencyBegan {
            president: self.president,
            special_election: special,
        });
    }
}
