//! `EmbeddingGreedyBot` tests (PRD-codenames-evals US-CN06), driven by the
//! hand-crafted fixture table in `crates/engine/fixtures/`. No pre-trained
//! asset is vendored in this phase, so the fixture *is* the vector source: 12
//! dimensions, eight semantic clusters of eight wordlist words each, plus
//! centroid and distractor clue candidates that are not on the wordlist.
//!
//! Because the fixture geometry is exact (see its header), every expectation
//! below is a single unambiguous right answer rather than a plausible one.
//! Every clue the bot emits is pushed through the real engine `apply()`, which
//! is the only legality authority.
//!
//! The table the bot reads from is tested next door in `codenames_vectors.rs`:
//! parsing strictness and cosine belong to `VectorTable`, not to the bot.

use engine::codenames::actions::{Action, DecisionPoint};
use engine::codenames::agents::{EmbeddingGreedyBot, RandomLegalBot, SeatAgent, VectorTable};
use engine::codenames::board::Card;
use engine::codenames::state::{GameState, Phase};
use engine::codenames::types::*;
use engine::codenames::wordlist::Wordlist;
use engine::game_core::{resolve_decision, Reliability};
use std::sync::Arc;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/codenames_test_vectors.txt"
);

fn table() -> Arc<VectorTable> {
    Arc::new(VectorTable::from_path(FIXTURE).expect("fixture vector table parses"))
}

/// The vendored wordlist narrowed to words the fixture can score — the pool
/// every board in these tests is drawn from, so no card is ever untabled.
fn covered_wordlist(table: &VectorTable) -> Wordlist {
    let text: String = Wordlist::default_embedded()
        .words()
        .iter()
        .filter(|w| table.contains(w))
        .map(|w| format!("{w}\n"))
        .collect();
    Wordlist::parse(&text).expect("the covered pool is a valid wordlist")
}

/// A game whose board is dictated card by card, written as `word:identity`
/// pairs — `a` own agent (team A), `b` enemy agent, `-` bystander, `x`
/// assassin. The 9/8/7/1 composition is checked here so a mis-typed layout
/// fails loudly instead of quietly changing what the bot is being asked.
fn crafted_game(table: &VectorTable, layout: &str) -> GameState {
    let cards: Vec<Card> = layout
        .split_whitespace()
        .map(|entry| {
            let (word, tag) = entry.split_once(':').expect("word:identity");
            Card {
                word: word.to_string(),
                identity: match tag {
                    "a" => CardIdentity::Agent { team: Team::A },
                    "b" => CardIdentity::Agent { team: Team::B },
                    "-" => CardIdentity::Bystander,
                    "x" => CardIdentity::Assassin,
                    other => panic!("unknown identity tag {other:?}"),
                },
                revealed: false,
            }
        })
        .collect();
    assert_eq!(cards.len(), CARD_COUNT, "a board is 25 cards");
    let count = |want: CardIdentity| cards.iter().filter(|c| c.identity == want).count();
    assert_eq!(count(CardIdentity::Agent { team: Team::A }), 9);
    assert_eq!(count(CardIdentity::Agent { team: Team::B }), 8);
    assert_eq!(count(CardIdentity::Bystander), 7);
    assert_eq!(count(CardIdentity::Assassin), 1);

    let mut state = GameState::with_wordlist(1, covered_wordlist(table), GameConfig::default());
    state.board.cards = cards;
    state
}

/// One clean own cluster (music: guitar/piano/violin); every other cluster
/// holding an own agent is contaminated by an enemy or a bystander.
const CLEAN_MUSIC_BOARD: &str = "\
    guitar:a piano:a violin:a dog:a ocean:a bread:a moon:a castle:a train:a \
    cat:b river:b cheese:b church:b tiger:b lake:b pizza:b tower:b \
    wolf:- star:- truck:- hammer:- drill:- banana:- comet:- shovel:x";

/// The single decision the engine is waiting on.
fn pending(state: &GameState) -> DecisionPoint {
    let mut decisions = state.pending_decisions();
    assert_eq!(decisions.len(), 1, "codenames is fully sequential");
    decisions.remove(0)
}

/// Ask the bot for the action it wants at the pending decision, without
/// applying it.
async fn proposed(bot: &mut EmbeddingGreedyBot, state: &GameState) -> Action {
    let decision = pending(state);
    let obs = state.observe(decision.seat());
    bot.decide(&obs, &decision, None)
        .await
        .expect("the bot never errors")
        .action
}

/// Ask, then push it through the engine — the real legality check.
async fn play_one(bot: &mut EmbeddingGreedyBot, state: &mut GameState) -> Action {
    let decision = pending(state);
    let action = proposed(bot, state).await;
    state
        .apply(decision.seat(), action.clone())
        .unwrap_or_else(|e| panic!("engine rejected {action:?}: {e}"));
    action
}

#[tokio::test]
async fn the_spymaster_clues_the_clean_cluster_and_sizes_the_number_to_it() {
    let table = table();
    let mut state = crafted_game(&table, CLEAN_MUSIC_BOARD);
    let mut bot = EmbeddingGreedyBot::new(Arc::clone(&table));

    let action = play_one(&mut bot, &mut state).await;
    assert_eq!(
        action,
        Action::GiveClue {
            word: "music".into(),
            number: 3
        },
        "the only uncontaminated own cluster is guitar/piano/violin"
    );
    assert_eq!(state.current_clue().map(|c| c.number), Some(3));

    // Deterministic: the same observation yields the same clue, always.
    let mut fresh = crafted_game(&table, CLEAN_MUSIC_BOARD);
    let mut other = EmbeddingGreedyBot::new(Arc::clone(&table));
    for _ in 0..3 {
        assert_eq!(proposed(&mut other, &fresh).await, action);
    }
    // Revealing the cluster shrinks the clue rather than repeating it.
    for word in ["guitar", "piano"] {
        let index = fresh.board.find_word(word).expect("on the board");
        fresh.board.cards[index].revealed = true;
    }
    assert_eq!(
        proposed(&mut other, &fresh).await,
        Action::GiveClue {
            word: "music".into(),
            number: 1
        }
    );
}

#[tokio::test]
async fn a_clue_sitting_near_the_assassin_is_rejected_however_tempting() {
    let table = table();
    // dog/cat/wolf are three own agents in one cluster — the best clue on the
    // board by cluster size — but `tiger` is the assassin.
    let layout = "\
        dog:a cat:a wolf:a guitar:a piano:a ocean:a bread:a castle:a train:a \
        river:b lake:b cheese:b pizza:b church:b tower:b truck:b taxi:b \
        moon:- star:- comet:- hammer:- drill:- shovel:- wrench:- tiger:x";
    let mut state = crafted_game(&table, layout);
    let mut bot = EmbeddingGreedyBot::new(Arc::clone(&table));

    let action = play_one(&mut bot, &mut state).await;
    let Action::GiveClue { word, number } = &action else {
        panic!("a spymaster gives clues, got {action:?}");
    };
    assert_eq!((word.as_str(), *number), ("music", 2), "the safe cluster");
    assert!(
        table.similarity(word, "tiger").expect("tabled") < bot.tuning().assassin_reject,
        "{word:?} sits on top of the assassin"
    );
    // The tempting clues really were on the table and really were dropped.
    for tempting in ["animal", "beast"] {
        assert!(table.similarity(tempting, "tiger").expect("tabled") > 0.9);
        assert!(table.similarity(tempting, "dog").expect("tabled") > 0.9);
    }
}

#[tokio::test]
async fn the_spymaster_never_emits_a_clue_the_engine_would_reject() {
    let table = table();
    // Both pools: the covered pool (every card scorable) and the full vendored
    // wordlist, where most cards have no vector at all and clue candidates
    // routinely collide with the board.
    for pool in [covered_wordlist(&table), Wordlist::default_embedded()] {
        for seed in 0..40u64 {
            let mut state = GameState::with_wordlist(seed, pool.clone(), GameConfig::default());
            let mut bot = EmbeddingGreedyBot::new(Arc::clone(&table));
            let action = proposed(&mut bot, &state).await;
            let Action::GiveClue { word, number } = &action else {
                panic!("expected a clue, got {action:?}");
            };
            let face_down = state.board.unrevealed_words();
            for board_word in &face_down {
                assert!(
                    !(board_word.contains(word) || word.contains(board_word)),
                    "seed {seed}: clue {word:?} overlaps face-down {board_word:?}"
                );
            }
            assert!(*number >= 1 && *number <= state.remaining_agents(Team::A));
            let seat = pending(&state).seat();
            state
                .apply(seat, action.clone())
                .unwrap_or_else(|e| panic!("seed {seed}: engine rejected {action:?}: {e}"));
        }
    }

    // The clue-length cap is config-driven, so it is read off the observation:
    // a game configured tighter than the default narrows the candidate set
    // instead of producing a clue the engine would refuse.
    let mut state = crafted_game(&table, CLEAN_MUSIC_BOARD);
    state.config.clue_word_max_len = 4;
    let mut bot = EmbeddingGreedyBot::new(Arc::clone(&table));
    let action = play_one(&mut bot, &mut state).await;
    let Action::GiveClue { word, number } = &action else {
        panic!("expected a clue, got {action:?}");
    };
    assert!(word.chars().count() <= 4, "{word:?} busts the cap");
    assert_eq!(*number, 3, "a shorter clue, still the music cluster");
}

#[tokio::test]
async fn the_operative_walks_the_clue_cluster_in_order_and_then_passes() {
    let table = table();
    let mut state = crafted_game(&table, CLEAN_MUSIC_BOARD);
    let mut spymaster = EmbeddingGreedyBot::new(Arc::clone(&table));
    let mut operative = EmbeddingGreedyBot::new(Arc::clone(&table));
    play_one(&mut spymaster, &mut state).await;

    // All three cluster words are equally similar to `music`, so the tie-break
    // is lexicographic — and all three are own agents, so the turn continues.
    for expected in ["guitar", "piano", "violin"] {
        assert_eq!(
            play_one(&mut operative, &mut state).await,
            Action::Guess {
                word: expected.into()
            }
        );
    }
    // The bonus guess is declined: the clue's number is the contract.
    assert_eq!(play_one(&mut operative, &mut state).await, Action::Pass);
    assert_eq!(state.active, Team::B);
    assert_eq!(state.remaining_agents(Team::A), 6, "three own agents found");
    assert_eq!(state.clues[0].outcomes.len(), 3);
    assert!(state.clues[0].passed);
}

#[tokio::test]
async fn an_untabled_clue_word_still_gets_its_mandatory_guess_deterministically() {
    let table = table();
    let mut state = crafted_game(&table, CLEAN_MUSIC_BOARD);
    assert!(!table.contains("kettle"), "an untabled but legal clue word");
    // `bread` is the lexicographically first face-down word once the bystander
    // ahead of it is gone — and it is an own agent, so the turn survives the
    // mandatory guess and the pass decision actually gets asked.
    let index = state.board.find_word("banana").expect("on the board");
    state.board.cards[index].revealed = true;
    state
        .apply(
            SEAT_A_SPYMASTER,
            Action::GiveClue {
                word: "kettle".into(),
                number: 2,
            },
        )
        .expect("legal clue");

    let mut operative = EmbeddingGreedyBot::new(Arc::clone(&table));
    // Lexicographically first face-down word — deterministic, never random.
    assert_eq!(
        play_one(&mut operative, &mut state).await,
        Action::Guess {
            word: "bread".into()
        }
    );
    assert!(matches!(state.phase, Phase::Guess { .. }), "turn continues");
    assert_eq!(
        proposed(&mut operative, &state).await,
        Action::Pass,
        "with nothing to rank, one mandatory guess is all it will risk"
    );
}

/// Which team a bot family sits on, so the starting-team advantage is mirrored
/// rather than handed to the anchor.
fn seat_agents(
    embedding_team: Team,
    table: &Arc<VectorTable>,
    seed: u64,
) -> Vec<Box<dyn SeatAgent>> {
    (0..SEAT_COUNT)
        .map(|seat| {
            if seat_team(seat) == embedding_team {
                Box::new(EmbeddingGreedyBot::new(Arc::clone(table))) as Box<dyn SeatAgent>
            } else {
                Box::new(RandomLegalBot::new(seed ^ (u64::from(seat) << 8))) as Box<dyn SeatAgent>
            }
        })
        .collect()
}

/// Play one game to the end; every action is applied through the shared
/// rethink loop with a retry budget of 1, so a single illegal action from
/// either bot would surface as a forced default.
async fn play(
    table: &Arc<VectorTable>,
    embedding_team: Team,
    seed: u64,
) -> (GameState, Reliability) {
    let pool = covered_wordlist(table);
    let mut state = GameState::with_wordlist(seed, pool, GameConfig::default());
    let mut agents = seat_agents(embedding_team, table, seed);
    let mut reliability = Reliability::default();
    let mut steps = 0;

    while !state.is_over() {
        steps += 1;
        assert!(steps <= 200, "seed {seed}: game did not terminate");
        let decision = pending(&state);
        let seat = decision.seat();
        let outcome = resolve_decision(
            1,
            &mut state,
            &mut agents[seat as usize],
            &decision,
            &mut reliability,
        )
        .await;
        assert!(
            !outcome.forced,
            "seed {seed}: seat {seat} produced an action the engine rejected"
        );
    }
    (state, reliability)
}

/// The anchor has to be a real floor, not noise: over both seatings of a fixed
/// seed sweep it must beat `RandomLegalBot` decisively.
#[tokio::test]
async fn the_embedding_anchor_beats_the_random_floor_from_either_side() {
    let table = table();
    let mut wins = 0;
    let mut games = 0;
    for seed in 0..20u64 {
        for embedding_team in TEAMS {
            let (state, reliability) = play(&table, embedding_team, seed).await;
            assert_eq!(reliability.illegal_moves, 0, "seed {seed}");
            assert_eq!(reliability.forced_defaults, 0, "seed {seed}");
            games += 1;
            if state.winner == Some(embedding_team) {
                wins += 1;
            }
        }
    }
    assert_eq!(games, 40);
    assert!(
        wins * 100 >= games * 60,
        "the embedding anchor won only {wins}/{games} games"
    );
}

#[tokio::test]
async fn anchor_games_replay_bit_for_bit() {
    let transcript = |seed: u64| async move {
        let table = table();
        let (state, _) = play(&table, Team::A, seed).await;
        serde_json::to_string(&state.events).expect("events serialize")
    };
    for seed in [3u64, 11, 29] {
        assert_eq!(
            transcript(seed).await,
            transcript(seed).await,
            "seed {seed}"
        );
    }
    assert_ne!(transcript(3).await, transcript(11).await);

    let bot = EmbeddingGreedyBot::new(table());
    assert_eq!(bot.kind(), "bot:codenames-embedding");
    assert_eq!(bot.model_id(), "bot:codenames-embedding");
    assert_eq!(bot.scaffold_version(), "bot-embedding-v1");
    assert_eq!(bot.usage().prompt_tokens, 0);
    assert_eq!(bot.usage().completion_tokens, 0);
}
