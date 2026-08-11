//! Scripted-game helpers for the rules tests: a fixed pool with no substring
//! pairs, so tests assert exact clue legality instead of fishing through the
//! vendored wordlist (whose content is curated independently of the engine).

use super::actions::Action;
use super::state::GameState;
use super::types::{CardIdentity, GameConfig};
use super::wordlist::Wordlist;

/// 40 words, none a substring of another — 25 land on the board, the rest
/// stay available as legal clues.
pub const TEST_WORDS: &str = "\
alfa\nbravo\ncharlie\ndelta\necho\nfoxtrot\ngolf\nhotel\nindia\njuliett\n\
kilo\nlima\nmike\nnovember\noscar\npapa\nquebec\nromeo\nsierra\ntango\n\
uniform\nvictor\nwhiskey\nxray\nyankee\nzulu\napple\nbasket\ncastle\ndinner\n\
engine\nfarmer\ngarden\nhelmet\ninsect\njungle\nkettle\nlantern\nmarket\nneedle\n";

pub fn test_wordlist() -> Wordlist {
    Wordlist::parse(TEST_WORDS).expect("test wordlist is valid")
}

/// A game on the scripted pool with default rules knobs.
pub fn scripted_game(seed: u64) -> GameState {
    GameState::with_wordlist(seed, test_wordlist(), GameConfig::default())
}

/// The first face-down word with the given identity (grid order).
pub fn word_with(state: &GameState, identity: CardIdentity) -> String {
    words_with(state, identity)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no unrevealed {identity:?} left on the board"))
}

/// Every face-down word with the given identity, in grid order.
pub fn words_with(state: &GameState, identity: CardIdentity) -> Vec<String> {
    state
        .board
        .cards
        .iter()
        .filter(|c| !c.revealed && c.identity == identity)
        .map(|c| c.word.clone())
        .collect()
}

/// A pool word that is a legal clue on the current board.
pub fn legal_clue(state: &GameState) -> String {
    let max_len = state.config.clue_word_max_len;
    state
        .wordlist
        .words()
        .iter()
        .find(|w| super::transitions::clue::clue_word_is_legal(&state.board, w, max_len))
        .cloned()
        .expect("the scripted pool always leaves a legal clue")
}

/// Have the active team's spymaster give a legal clue for `number`.
pub fn give_clue(state: &mut GameState, number: u8) {
    let seat = state.active.spymaster();
    let word = legal_clue(state);
    state
        .apply(seat, Action::GiveClue { word, number })
        .expect("scripted clue is legal");
}

/// Flip a card without playing a turn — for setting up end-game positions.
pub fn reveal(state: &mut GameState, word: &str) {
    let index = state
        .board
        .find_word(word)
        .unwrap_or_else(|| panic!("{word} is not on the board"));
    state.board.cards[index].revealed = true;
}
