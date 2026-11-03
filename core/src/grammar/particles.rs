//! What a particle means *here*, when the sentence gives enough away.
//!
//! # Why this does not decide
//!
//! で marks where an action happens, what it is done with, what a thing is made of, why it
//! happened, and how much it takes — and 静かで is not で at all, but the て-form of だ. A
//! single blended gloss states that ambiguity rather than resolving it, which is what the
//! pack said before this existed.
//!
//! Resolving it properly means semantic role labeling over a dependency parse. The good
//! Japanese parsers are Python, which `project notes` §2 rules out. What is available is the
//! observation that a particle's sense is largely fixed by **the word in front of it** —
//! its part of speech, whether it is a counter, and what kind of thing it names.
//!
//! That is enough to *favour* a reading. It is not enough to pronounce one, so this
//! returns every sense, ranked, with the reason for the ranking. A learner shown three
//! possibilities and told which looks likeliest is better served than one shown a
//! confident wrong answer — and the difference has to stay visible in what is returned,
//! not be flattened into a single pick.
//!
//! # Where the categories come from
//!
//! [`Categories`] maps a noun to a coarse class — `noun.location`, `noun.artifact`,
//! `noun.substance`. It is built from WordNet by `scripts/fetch-wordnet.ps1` and is
//! **optional**: without it the structural cues still work and the app simply abstains
//! more often. Tier 2 is an improvement on Tier 1, never a dependency of it.
//!
//! It deliberately does not read `Token::senses`. Those are populated by Ichiran and left
//! empty by Lindera, so a rule reading them would work for a user with the sidecar and be
//! dead for everyone else — a mistake this project has already made and paid for once.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::grammar::pack::Example;
use crate::model::Token;

/// The bundled particle senses, as JSON.
pub const PARTICLES_JSON: &str = include_str!("packs/particles.json");

/// Everything known about one particle's readings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleEntry {
    pub surface: String,
    /// How it is typed on a keyboard that has no kana — `ni`, `wo` and `o`, `ha` and `wa`.
    ///
    /// Here rather than in a table in the code, because it is a fact about the particle
    /// and belongs beside its senses. More than one where the spelling and the sound
    /// disagree: は is typed `ha` and said `wa`, and a learner may reach for either.
    #[serde(default)]
    pub romaji: Vec<String>,
    #[serde(default)]
    pub note: String,
    pub senses: Vec<Sense>,
}

impl ParticleEntry {
    /// Whether this particle's readings can be ranked at all.
    ///
    /// **A particle whose ordinary reading has no cues cannot be ranked, only interrupted.** A
    /// cueless sense can never score above zero, and promotion needs a strictly-positive unique
    /// top score — so for を, が and は the plain object, plain subject and plain topic are
    /// unreachable by construction. The panel can therefore say one of two things: the marked
    /// reading, or nothing. It can never say "this is the ordinary one", which is the answer
    /// almost every occurrence actually has.
    ///
    /// Measured over 5,886 occurrences of real sentences: を promoted on 1.1%, が on 10.0%, は on
    /// 16.4% — and every promotion for が and は was the marked sense. What a reader met instead,
    /// the other 84–99% of the time, was the ambiguity panel. That is not a ranking; it is a
    /// shrug with two paragraphs attached.
    ///
    /// So ranking is offered only where every sense can win. で and に qualify today. This is
    /// derived rather than listed, so authoring cues for `ga-subject` makes が rankable with no
    /// second place to remember — and deleting a cue takes a particle out of the panel rather
    /// than quietly degrading it.
    ///
    /// **The senses themselves are untouched.** `search_particles` reads `all()` and still
    /// carries every reading of every particle to the Reference page; this decides only whether
    /// the Analysis panel tries to pick one. Without it, the grammar pack's own card shows —
    /// which is what every other particle in Japanese already does.
    pub fn rankable(&self) -> bool {
        self.senses.iter().all(|sense| !sense.cues.is_empty())
    }
}

/// One reading a particle can have.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sense {
    pub id: String,
    /// A few words a learner can scan — "where the action happens".
    pub label: String,
    pub gloss: String,
    /// The dictionary sense(s) this one answers to, as the exact line the entry prints —
    /// its glosses joined with `"; "`, which is what the interface draws and therefore what
    /// it can match on.
    ///
    /// **Keyed by the words, not by the sense's position.** JMdict reorders and inserts
    /// senses between releases, and an index would then point silently at the wrong
    /// reading; a line that stops matching fails loudly, and `tests/particle_glosses.rs`
    /// is what makes it fail.
    ///
    /// Empty is a legitimate answer and means the dictionary does not carve this reading
    /// out separately — で's "made of" and に's indirect object are both real uses with no
    /// sense of their own to point at. Those are gaps in the dictionary's carving, not in
    /// this file, and the interface shows the taught sense unchanged.
    #[serde(default)]
    pub dictionary: Vec<String>,
    #[serde(default)]
    pub cues: Vec<Cue>,
    /// Native sentences showing this sense, from the corpus rather than from me.
    ///
    /// The translation is what makes a sentence evidence: it is how a reader checks that
    /// the sentence really does show *this* reading and not one of the others. Each is
    /// picked by hand for that reason (`scripts/harvest-sentences.py --particle`).
    #[serde(default)]
    pub examples: Vec<Example>,
}

/// Evidence for one sense over the others.
///
/// Data rather than code, so adding a particle is editing JSON. Each carries a weight
/// because the cues are not equally telling: a counter before で is nearly conclusive,
/// where "the noun is an artifact" is only suggestive — a hammer is an artifact and so is
/// a library.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Cue {
    /// The token immediately after the particle is one of these.
    FollowedBy { surfaces: Vec<String>, weight: i32 },
    /// The token before carries one of these part-of-speech labels.
    ///
    /// Both spellings are retained because analyzer histories disagree: Ichiran reports
    /// JMdict's `adj-na`, while the former IPADIC adapter exposed `形容動詞語幹`. UniDic's
    /// ordinary 形状詞 currently carries no normalized conjugation class here, so the built-in
    /// analyzer conservatively contributes no preceding-POS evidence for that case.
    PrecedingPos { classes: Vec<String>, weight: i32 },
    /// The token before is a number or a counter.
    PrecedingCounter { weight: i32 },
    /// The token before is spelled as one of these.
    ///
    /// For a reading that belongs to particular words rather than to a class of them. に after
    /// 本当, 一緒 or 絶対 is the case particle — UD Japanese-GSD tags it 格助詞 every time — but
    /// it makes an adverb of the word, and nothing about the word's class says so: 本当 is an
    /// ordinary noun, and so is 日本 in 日本に行く.
    PrecedingSurface { surfaces: Vec<String>, weight: i32 },
    /// Whether the token before inflects — a verb or adjective rather than a thing.
    ///
    /// The one fact that separates に's destination from its purpose, because the verb
    /// *after* に is 行く in both: 駅に行く goes to a place, 買いに行く goes in order to
    /// buy. Only the word in front tells them apart, and "does it conjugate" is the
    /// question, not what it means.
    PrecedingConjugates { conjugates: bool, weight: i32 },
    /// The token before falls in one of these WordNet classes. Needs Tier 2; contributes
    /// nothing when the table is absent.
    PrecedingCategory {
        categories: Vec<String>,
        weight: i32,
    },
    /// A predicate anywhere later in the sentence is one of these.
    Predicate { surfaces: Vec<String>, weight: i32 },
    /// Another は appears elsewhere in the sentence.
    ///
    /// The only cue は gets, and a weak one. Topic against contrast is information
    /// structure — what the speaker assumes the listener already has in mind — and no
    /// amount of looking at the surrounding words settles it. Two は in one sentence is
    /// the one surface fact that leans the right way.
    SecondWa { weight: i32 },
}

impl Cue {
    fn weight(&self) -> i32 {
        match self {
            Cue::FollowedBy { weight, .. }
            | Cue::PrecedingPos { weight, .. }
            | Cue::PrecedingCounter { weight }
            | Cue::PrecedingSurface { weight, .. }
            | Cue::PrecedingConjugates { weight, .. }
            | Cue::PrecedingCategory { weight, .. }
            | Cue::Predicate { weight, .. }
            | Cue::SecondWa { weight } => *weight,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ParticlePack {
    particles: Vec<ParticleEntry>,
}

/// The bundled senses, parsed once.
pub fn all() -> &'static [ParticleEntry] {
    static PARSED: OnceLock<Vec<ParticleEntry>> = OnceLock::new();
    PARSED
        .get_or_init(|| {
            serde_json::from_str::<ParticlePack>(PARTICLES_JSON)
                .expect("the bundled particle pack parses")
                .particles
        })
        .as_slice()
}

/// The entry for a surface, if this is a particle with senses written for it.
pub fn entry(surface: &str) -> Option<&'static ParticleEntry> {
    all().iter().find(|entry| entry.surface == surface)
}

/// Particles a search should offer, best match first.
///
/// Deliberately narrow. It matches the particle itself and how it is typed, and nothing
/// about what it means — a reader who types `に` gets に, and one who types "location"
/// gets the dictionary, because matching English against the sense glosses would put a
/// particle at the top of every search containing "what" or "is".
///
/// Prefix-matched the way [`crate::terms::search`] is, and in the same direction: the
/// alias has to start with the query. So `n` finds に, and `desu` does not find で.
pub fn search(query: &str) -> Vec<&'static ParticleEntry> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }

    let mut found: Vec<&'static ParticleEntry> = Vec::new();
    let mut push = |entry: &'static ParticleEntry| {
        if !found.iter().any(|seen| seen.surface == entry.surface) {
            found.push(entry);
        }
    };

    // The particle itself first, so typing `に` cannot be outranked by a romaji prefix.
    if let Some(exact) = entry(&query) {
        push(exact);
    }
    for entry in all() {
        if entry.surface.starts_with(&query) || entry.romaji.iter().any(|r| r.starts_with(&query)) {
            push(entry);
        }
    }
    found
}

/// Noun → coarse semantic class, from `scripts/fetch-wordnet.ps1`.
///
/// Absent by default. Everything here works without it; it only lets more cues fire.
#[derive(Debug, Default)]
pub struct Categories(HashMap<String, String>);

impl Categories {
    /// Read the `word<TAB>category` table the fetch script writes.
    ///
    /// A missing file is not an error — it is the ordinary state of a fresh install, and
    /// the caller gets an empty table that simply never matches.
    pub fn load(path: &std::path::Path) -> Categories {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Categories::default();
        };
        let mut map = HashMap::new();
        for line in text.lines() {
            if let Some((word, category)) = line.split_once('\t') {
                map.insert(word.to_string(), category.trim().to_string());
            }
        }
        Categories(map)
    }

    pub fn get(&self, word: &str) -> Option<&str> {
        self.0.get(word).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A sense, with whatever the sentence had to say about it.
#[derive(Debug, Clone, Serialize)]
pub struct RankedSense {
    pub id: String,
    pub label: String,
    pub gloss: String,
    /// Higher is better supported. Zero means nothing pointed at it either way — which is
    /// not evidence against it, and the sense is still shown.
    pub score: i32,
    /// Why it scored, in words a learner can check against the sentence. Empty when
    /// nothing fired.
    pub because: Vec<String>,
    pub examples: Vec<Example>,
}

/// Every sense of one particle, and which of them the sentence points at.
#[derive(Debug, Clone, Serialize)]
pub struct Reading {
    /// Best-supported first. **Never filtered** — the ranking reorders and nothing else.
    pub senses: Vec<RankedSense>,
    /// Index into [`senses`](Reading::senses) of the reading the sentence favours.
    ///
    /// `None` covers two situations the interface must not confuse with a pick: nothing
    /// fired at all, and *a tie at the top*. A tie is not a favourite — ranking exists to
    /// say "the words around it lean this way", and two senses on the same score means
    /// they do not. Promoting the first because it happens to be authored first would be
    /// exactly the confident wrong answer this module was built to avoid.
    ///
    /// It is computed here rather than left to the caller so the panel and the scored
    /// fixture cannot drift apart about what "favoured" means. A test measuring a rule
    /// the reader never sees measures nothing.
    pub favoured: Option<usize>,
}

/// Every sense of the particle at `index`, best-supported first.
///
/// Returns `None` when the token is not a particle this knows about — the caller then
/// falls back to whatever the grammar pack says, exactly as before.
///
/// **The scores can all be zero**, and that is a designed outcome rather than a failure:
/// it means the sentence gave nothing away and the learner should see the possibilities
/// rather than a guess. [`Reading::favoured`] is what tells the interface which of those
/// two situations it is in.
pub fn senses(tokens: &[Token], index: usize, categories: &Categories) -> Option<Reading> {
    let token = tokens.get(index)?;
    let entry = entry(&token.surface)?;
    // Not every authored particle can be ranked — see `ParticleEntry::rankable`. `None` here is
    // the same `None` every unauthored particle returns, and the caller already falls back to the
    // grammar pack for it.
    if !entry.rankable() {
        return None;
    }

    let mut ranked: Vec<RankedSense> = entry
        .senses
        .iter()
        .map(|sense| {
            let mut score = 0;
            let mut because = Vec::new();
            for cue in &sense.cues {
                if let Some(reason) = fires(cue, tokens, index, categories) {
                    score += cue.weight();
                    because.push(reason);
                }
            }
            RankedSense {
                id: sense.id.clone(),
                label: sense.label.clone(),
                gloss: sense.gloss.clone(),
                score,
                because,
                examples: sense.examples.clone(),
            }
        })
        .collect();

    // Stable within a score, so senses keep their authored order when nothing separates
    // them — the order they are written in is the order a learner should meet them.
    ranked.sort_by_key(|sense| std::cmp::Reverse(sense.score));

    let favoured = match ranked.as_slice() {
        [best, ..] if best.score <= 0 => None,
        [_] => Some(0),
        [best, second, ..] if second.score < best.score => Some(0),
        _ => None,
    };
    Some(Reading {
        senses: ranked,
        favoured,
    })
}

/// Whether a cue fires, and how to say so.
fn fires(cue: &Cue, tokens: &[Token], index: usize, categories: &Categories) -> Option<String> {
    let before = index.checked_sub(1).and_then(|i| tokens.get(i));

    match cue {
        Cue::FollowedBy { surfaces, .. } => {
            let next = tokens.get(index + 1)?;
            surfaces
                .iter()
                .any(|s| s == next.surface.trim() || *s == next.surface)
                .then(|| "a comma follows it".to_string())
        }

        Cue::PrecedingPos { classes, .. } => {
            let token = before?;
            let matched = token
                .conjugation
                .iter()
                .flat_map(|conjugation| conjugation.props.iter())
                .any(|prop| {
                    classes
                        .iter()
                        .any(|class| prop.pos.contains(class.as_str()))
                });
            matched.then(|| format!("{} is a な-adjective", token.surface))
        }

        Cue::PrecedingCounter { .. } => {
            let token = before?;
            is_counted(&token.surface).then(|| format!("{} is an amount", token.surface))
        }

        Cue::PrecedingSurface { surfaces, .. } => {
            let token = before?;
            surfaces.contains(&token.surface).then(|| {
                format!(
                    "{} is one of the words に turns into an adverb",
                    token.surface
                )
            })
        }

        Cue::PrecedingConjugates { conjugates, .. } => {
            let token = before?;
            let inflects = token
                .conjugation
                .as_ref()
                .is_some_and(|conjugation| !conjugation.props.is_empty());
            (inflects == *conjugates).then(|| {
                if inflects {
                    format!("{} is a verb, not a thing", token.surface)
                } else {
                    format!("{} is a thing, not a verb", token.surface)
                }
            })
        }

        Cue::PrecedingCategory {
            categories: wanted, ..
        } => {
            let token = before?;
            let term = if token.dict_form.is_empty() {
                &token.surface
            } else {
                &token.dict_form
            };
            let found = categories.get(term)?;
            wanted
                .iter()
                .any(|c| c == found)
                .then(|| format!("{} is {}", token.surface, plain_english(found)))
        }

        Cue::Predicate { surfaces, .. } => {
            // **Only as far as the end of this sentence.** This used to scan to the end of the
            // token list, and the interface hands over the whole analysed passage — so a verb in
            // the *next* sentence decided this one's particle. Measured over consecutive
            // sentences, that moved 35.7% of answers in a five-sentence capture and touched
            // 98.2% of captures: `猫が庭にいる。` became "what is liked, wanted or understood"
            // because a later sentence about a dictionary contained わかる.
            let (_, end) = crate::model::sentence_bounds(tokens, index);
            let rest = tokens.get(index + 1..end)?;
            for (offset, token) in rest.iter().enumerate() {
                for wanted in surfaces {
                    if token.dict_form == *wanted || token.surface == *wanted {
                        return Some(format!("the verb is {}", token.dict_form_or_surface()));
                    }
                    // A する-compound arrives split under Lindera — 勉強する as 勉強 | する —
                    // so an entry written the way a dictionary writes it matches nothing.
                    // Ichiran fuses it into one token, which is exactly why this survived:
                    // every する-verb in every cue list was live for sidecar users and dead
                    // for everyone else. The same shape of bug as reading `Token::senses`.
                    if let Some(stem) = wanted.strip_suffix("する") {
                        let suru_follows = rest
                            .get(offset + 1)
                            .map(|next| next.dict_form == "する")
                            .unwrap_or(false);
                        if suru_follows && (token.dict_form == stem || token.surface == stem) {
                            return Some(format!("the verb is {wanted}"));
                        }
                    }
                }
            }
            None
        }

        Cue::SecondWa { .. } => {
            // In *this* sentence, which is what the reason string has always claimed. Counting
            // across the passage made every は contrastive the moment a second sentence carried
            // one, which is nearly always: 18 of 20 sampled neighbours flipped ~80% of は answers.
            let (start, end) = crate::model::sentence_bounds(tokens, index);
            let others = tokens
                .get(start..end)
                .unwrap_or_default()
                .iter()
                .enumerate()
                .filter(|(offset, token)| start + offset != index && token.surface == "は")
                .count();
            (others > 0).then(|| "there is another は in the sentence".to_string())
        }
    }
}

/// Whether a surface reads as a number with a counter — 三日, 5人, 十時間.
///
/// Deliberately crude: a digit or a kanji numeral followed by anything. Getting this
/// slightly wrong costs a cue, and the cue only ever adds weight to a sense that is shown
/// regardless.
fn is_counted(surface: &str) -> bool {
    let mut chars = surface.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let numeric = first.is_ascii_digit()
        || matches!(first, '０'..='９')
        || matches!(
            first,
            '一' | '二'
                | '三'
                | '四'
                | '五'
                | '六'
                | '七'
                | '八'
                | '九'
                | '十'
                | '百'
                | '千'
                | '万'
                | '何'
        );
    numeric && surface.chars().count() > 1
}

/// A WordNet class name as something to read.
fn plain_english(category: &str) -> &str {
    match category {
        "noun.location" => "a place",
        "noun.artifact" => "a made thing — a tool, a vehicle",
        "noun.substance" => "a material",
        "noun.time" => "a time",
        "noun.quantity" => "an amount",
        "noun.event" => "an event",
        "noun.state" => "a state",
        "noun.phenomenon" => "something that happens",
        "noun.person" => "a person",
        other => other,
    }
}

impl Token {
    fn dict_form_or_surface(&self) -> &str {
        if self.dict_form.is_empty() {
            &self.surface
        } else {
            &self.dict_form
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn every_particle_says_how_it_is_typed() {
        for entry in all() {
            assert!(
                !entry.romaji.is_empty(),
                "{} has no romaji, so it can only be found by someone who can already                  type kana — which is not who is looking it up",
                entry.surface
            );
            for spelling in &entry.romaji {
                assert!(
                    spelling.chars().all(|c| c.is_ascii_lowercase()),
                    "{:?} is not plain lowercase ascii, and the search lowercases the query                      before comparing",
                    spelling
                );
            }
        }
    }

    #[test]
    fn a_particle_is_found_by_its_own_spelling_and_by_its_romaji() {
        for entry in all() {
            let by_surface = search(&entry.surface);
            assert_eq!(
                by_surface.first().map(|e| &e.surface),
                Some(&entry.surface),
                "{} does not lead its own search",
                entry.surface
            );
            for spelling in &entry.romaji {
                assert!(
                    search(spelling).iter().any(|e| e.surface == entry.surface),
                    "{} is not found by {spelling:?}",
                    entry.surface
                );
            }
        }
    }

    #[test]
    fn the_query_is_the_prefix_and_not_the_other_way_round() {
        // `de` is how で is typed, and `desu` is a word someone might search for. If the
        // comparison ran the other way every English word beginning with a romaji spelling
        // would drag a particle to the top of the results.
        assert!(search("de").iter().any(|e| e.surface == "で"));
        assert!(search("desu").is_empty(), "desu is not a particle query");
        for ordinary in ["eat", "hand", "onto", "water", "grammar"] {
            assert!(
                search(ordinary).is_empty(),
                "{ordinary:?} pulled up a particle"
            );
        }
    }

    #[test]
    fn a_single_letter_narrows_rather_than_matching_everything() {
        let n = search("n");
        assert_eq!(n.len(), 1, "n should reach に alone");
        assert_eq!(n[0].surface, "に");
        // わ and は share a sound, so `w` reaching both を and は is correct rather than sloppy.
        let w: Vec<&str> = search("w").iter().map(|e| e.surface.as_str()).collect();
        assert_eq!(w, ["を", "は"]);
    }

    #[test]
    fn an_empty_query_asks_nothing_and_gets_nothing() {
        for nothing in ["", "   ", "	"] {
            assert!(search(nothing).is_empty());
        }
    }

    #[test]
    fn searching_is_case_insensitive_and_ignores_surrounding_space() {
        for written in ["NI", "  ni  ", "Ni"] {
            assert_eq!(
                search(written).first().map(|e| e.surface.as_str()),
                Some("に"),
                "{written:?} should find に"
            );
        }
    }
    use super::*;

    fn token(surface: &str) -> Token {
        Token {
            surface: surface.into(),
            dict_form: surface.into(),
            ..Default::default()
        }
    }

    fn sentence(surfaces: &[&str]) -> Vec<Token> {
        surfaces.iter().map(|s| token(s)).collect()
    }

    fn de_reading(tokens: &[Token], categories: &Categories) -> Reading {
        let index = tokens.iter().position(|t| t.surface == "で").expect("a で");
        senses(tokens, index, categories).expect("で has senses")
    }

    fn de_at(tokens: &[Token], categories: &Categories) -> Vec<RankedSense> {
        de_reading(tokens, categories).senses
    }

    #[test]
    fn the_bundled_particle_pack_parses_and_every_sense_says_something() {
        let entries = all();
        assert!(!entries.is_empty());
        for entry in entries {
            assert!(!entry.senses.is_empty(), "{} has no senses", entry.surface);
            for sense in &entry.senses {
                assert!(!sense.id.is_empty());
                assert!(!sense.label.is_empty(), "{} has no label", sense.id);
                assert!(
                    sense.gloss.len() > 20,
                    "{} needs a gloss a learner can use",
                    sense.id
                );
            }
            let mut ids: Vec<&str> = entry.senses.iter().map(|s| s.id.as_str()).collect();
            ids.sort_unstable();
            let count = ids.len();
            ids.dedup();
            assert_eq!(ids.len(), count, "{} repeats a sense id", entry.surface);
        }
    }

    /// Every sense is illustrated, and by a sentence that contains the thing illustrated.
    ///
    /// The second half is the part worth asserting. An example of で's material sense that
    /// does not contain で is not a weaker example, it is not an example — and a typo in a
    /// hand-curated list is exactly how that happens.
    #[test]
    fn every_sense_is_shown_with_native_sentences_that_contain_the_particle() {
        for entry in all() {
            for sense in &entry.senses {
                assert!(
                    sense.examples.len() >= 2,
                    "{} needs examples; a learner choosing between readings needs to see \
                     each one used",
                    sense.id
                );
                for example in &sense.examples {
                    assert!(
                        example.japanese.contains(&entry.surface),
                        "{}: {} does not contain {}",
                        sense.id,
                        example.japanese,
                        entry.surface
                    );
                    assert!(
                        !example.english.is_empty(),
                        "{}: {} has no translation, so nothing shows which sense it is",
                        sense.id,
                        example.japanese
                    );
                }
            }
        }
    }

    /// **A particle whose ordinary reading has no cues is not ranked at all.**
    ///
    /// This test used to use は to show that a level field has no favourite — 父は会社員だ leaves
    /// both senses on zero, because `wa-contrast`'s cue is the only one either carries. That is
    /// still true, and it is now the reason は is not offered to the panel: the level field is not
    /// the exception for it but the ordinary case, on 83.6% of real occurrences.
    ///
    /// `wo-object`, `ga-subject` and `wa-topic` are cueless, so their particles can only ever
    /// promote a marked reading or say nothing. で and に have a cue on every sense and stay.
    #[test]
    fn a_particle_whose_common_reading_cannot_win_is_not_ranked() {
        let level = sentence(&["父", "は", "会社員", "だ"]);
        assert!(
            senses(&level, 1, &Categories::default()).is_none(),
            "は is not ranked; the grammar pack's own card answers for it"
        );

        for surface in ["は", "が", "を"] {
            let entry = entry(surface).expect("authored");
            assert!(
                !entry.rankable(),
                "{surface} has a cueless sense and cannot be ranked"
            );
        }
        for surface in ["で", "に"] {
            let entry = entry(surface).expect("authored");
            assert!(
                entry.rankable(),
                "{surface} has a cue on every sense and is ranked"
            );
        }

        // The senses themselves are untouched — the Reference page still lists all of them.
        assert_eq!(entry("は").expect("authored").senses.len(), 2);
        assert_eq!(entry("が").expect("authored").senses.len(), 2);
    }

    /// A cue may not read across a full stop.
    ///
    /// `Predicate` scanned to the end of the token list, and the interface passes the whole
    /// analysed passage — so a verb in a later sentence decided this one's particle. Measured
    /// over consecutive corpus sentences it moved 35.7% of answers in a five-sentence capture.
    #[test]
    fn a_verb_in_the_next_sentence_does_not_decide_this_one() {
        // 公園で走る — the verb makes it a path/place reading within its own sentence.
        let alone = sentence(&["公園", "で", "走る", "。"]);
        let within = sentence(&[
            "公園", "で", "走る", "。", "彼", "は", "手紙", "を", "作る", "。",
        ]);
        let one = senses(&alone, 1, &Categories::default()).expect("で is ranked");
        let two = senses(&within, 1, &Categories::default()).expect("で is ranked");
        assert_eq!(
            one.favoured.map(|at| one.senses[at].id.clone()),
            two.favoured.map(|at| two.senses[at].id.clone()),
            "the second sentence's 作る must not reach the first sentence's で"
        );
        // And specifically: `de-material` keys on 作る, which is only in the second sentence.
        let material = two
            .senses
            .iter()
            .find(|sense| sense.id == "de-material")
            .expect("de-material exists");
        assert_eq!(
            material.score, 0,
            "作る is in the next sentence and must not count"
        );
    }

    #[test]
    fn a_cue_that_fires_puts_one_reading_in_front() {
        let tokens = sentence(&["静か", "で", "、", "きれい", "だ"]);
        let reading = de_reading(&tokens, &Categories::default());
        assert_eq!(reading.favoured, Some(0));
        assert_eq!(reading.senses[0].id, "de-copula-te");
    }

    #[test]
    fn a_word_that_is_not_a_particle_has_nothing_to_say() {
        let tokens = sentence(&["本", "を", "読む"]);
        assert!(senses(&tokens, 0, &Categories::default()).is_none());
    }

    /// The case that most reliably confuses people: 静かで is not the particle.
    #[test]
    fn de_before_a_comma_is_the_copula_not_the_particle() {
        let tokens = sentence(&["静か", "で", "、", "きれい", "だ"]);
        let ranked = de_at(&tokens, &Categories::default());
        assert_eq!(
            ranked[0].id,
            "de-copula-te",
            "a comma after で should favour the copula, got {:?}",
            ranked.iter().map(|s| (&s.id, s.score)).collect::<Vec<_>>()
        );
        assert!(!ranked[0].because.is_empty(), "it has to say why");
    }

    #[test]
    fn an_amount_before_de_favours_the_limit() {
        let tokens = sentence(&["三日", "で", "終わる"]);
        let ranked = de_at(&tokens, &Categories::default());
        assert_eq!(ranked[0].id, "de-limit");
    }

    /// Tier 1 alone, which is what a fresh install has.
    ///
    /// Every sense is still offered, the structural cues still fire, and where nothing
    /// structural applies it abstains rather than guessing. That last part is what makes
    /// the WordNet table an improvement rather than a dependency.
    #[test]
    fn everything_still_works_with_no_category_table() {
        let studying = sentence(&["図書館", "で", "勉強", "する"]);
        let reading = de_reading(&studying, &Categories::default());
        assert_eq!(reading.senses.len(), 6, "every sense is offered regardless");
        assert_eq!(
            reading.senses[reading.favoured.expect("the verb alone is enough")].id,
            "de-place",
            "you study at a library, and no category table is needed to know it"
        );

        // And with a verb that says nothing either way, nothing is claimed.
        let idling = sentence(&["図書館", "で", "騒ぐ"]);
        let reading = de_reading(&idling, &Categories::default());
        assert_eq!(reading.senses.len(), 6);
        assert_eq!(reading.favoured, None);
    }

    /// Tier 2 is an improvement on that, not a replacement.
    #[test]
    fn a_place_before_de_favours_the_location_once_categories_are_installed() {
        let mut map = HashMap::new();
        map.insert("図書館".to_string(), "noun.location".to_string());
        let categories = Categories(map);

        let tokens = sentence(&["図書館", "で", "勉強", "する"]);
        let ranked = de_at(&tokens, &categories);
        assert_eq!(ranked[0].id, "de-place");
        assert!(ranked[0].because.iter().any(|why| why.contains("place")));
    }

    #[test]
    fn a_vehicle_before_de_favours_the_means() {
        let mut map = HashMap::new();
        map.insert("電車".to_string(), "noun.artifact".to_string());
        let categories = Categories(map);

        let tokens = sentence(&["電車", "で", "行く"]);
        assert_eq!(de_at(&tokens, &categories)[0].id, "de-means");
    }

    /// Nothing is ever hidden. Ranking reorders; it must not filter.
    #[test]
    fn every_sense_survives_ranking() {
        let mut map = HashMap::new();
        map.insert("図書館".to_string(), "noun.location".to_string());
        let categories = Categories(map);

        let authored = entry("で").expect("で").senses.len();
        let tokens = sentence(&["図書館", "で", "勉強", "する"]);
        assert_eq!(de_at(&tokens, &categories).len(), authored);
    }

    /// The case a category alone gets wrong.
    ///
    /// WordNet files a library under `noun.artifact` — a building is a made thing before
    /// it is a place — so leaning on the category would read 図書館で as "by means of a
    /// library". The verb is what separates a building from a tool, and it costs nothing.
    #[test]
    fn a_building_is_a_place_when_the_verb_says_so() {
        let mut map = HashMap::new();
        map.insert("図書館".to_string(), "noun.artifact".to_string());
        map.insert("電車".to_string(), "noun.artifact".to_string());
        let categories = Categories(map);

        let studying = sentence(&["図書館", "で", "勉強する"]);
        assert_eq!(
            de_at(&studying, &categories)[0].id,
            "de-place",
            "you study *at* a library, whatever WordNet files it under"
        );

        let travelling = sentence(&["電車", "で", "行く"]);
        assert_eq!(de_at(&travelling, &categories)[0].id, "de-means");
    }

    /// And when the verb says nothing, neither wins — which is the designed outcome.
    ///
    /// This asserted `score <= 1` when it was written, which tolerated exactly the bug the
    /// scored fixture later found: an `noun.artifact` cue on de-means at weight 1, standing
    /// alone, and therefore *favoured*. A score of 1 is not "nothing asserted" — it is a
    /// pick. The assertion is now on `favoured`, which is the thing the panel actually acts
    /// on, so it can no longer pass while the reader is told something.
    #[test]
    fn an_artifact_with_an_unhelpful_verb_leaves_it_open() {
        let mut map = HashMap::new();
        map.insert("図書館".to_string(), "noun.artifact".to_string());
        let categories = Categories(map);

        let tokens = sentence(&["図書館", "で", "騒ぐ"]);
        let reading = de_reading(&tokens, &categories);
        assert_eq!(
            reading.favoured,
            None,
            "nothing here separates place from means: {:?}",
            reading
                .senses
                .iter()
                .map(|s| (&s.id, s.score))
                .collect::<Vec<_>>()
        );
    }

    /// A する-compound reaches the cue in two pieces, and must still match.
    ///
    /// The plan's own headline sentence — 学校で勉強する — abstained when this was first
    /// wired up, because Lindera hands over 勉強 | する and the pack lists 勉強する. Under
    /// Ichiran it worked, which is the tell: a rule that is alive with the sidecar and
    /// dead without it. Both are asserted here so it cannot come back on one side only.
    #[test]
    fn a_suru_compound_matches_whether_the_analyzer_splits_it_or_not() {
        let categories = Categories::default();

        let split = sentence(&["学校", "で", "勉強", "する"]);
        let reading = senses(&split, 1, &categories).expect("で has senses");
        let at = reading
            .favoured
            .expect("勉強する is a thing you do somewhere");
        assert_eq!(reading.senses[at].id, "de-place");

        let fused = sentence(&["学校", "で", "勉強する"]);
        let reading = senses(&fused, 1, &categories).expect("で has senses");
        let at = reading.favoured.expect("and it reads the same fused");
        assert_eq!(reading.senses[at].id, "de-place");
    }

    /// The に pair 行く cannot separate.
    ///
    /// 駅に行く and 買いに行く end the same way. The only difference is in front of に, and
    /// it is not what the word *means* — it is whether it inflects at all.
    #[test]
    fn a_verb_stem_before_ni_is_a_purpose_and_a_noun_is_a_destination() {
        let categories = Categories::default();

        let mut buying = sentence(&["下着", "を", "買い", "に", "行く"]);
        buying[2].conjugation = Some(crate::model::Conjugation {
            lemma: "買う".into(),
            reading: "かう".into(),
            props: vec![crate::model::Prop {
                pos: "五段・ワ行促音便".into(),
                form: "Continuative".into(),
                ..Default::default()
            }],
        });
        let at = 3;
        let reading = senses(&buying, at, &categories).expect("に has senses");
        assert_eq!(
            reading.senses[reading.favoured.expect("a favourite")].id,
            "ni-purpose"
        );

        // A place before it, and the place is what decides. 行く merely confirms: it
        // follows に in the purpose reading too, so on its own it separates nothing.
        let mut map = HashMap::new();
        map.insert("公園".to_string(), "noun.location".to_string());
        let park = sentence(&["公園", "に", "行く"]);
        let reading = senses(&park, 1, &Categories(map)).expect("に has senses");
        assert_eq!(
            reading.senses[reading.favoured.expect("a favourite")].id,
            "ni-destination"
        );

        // And with no category table and nothing but 行く, neither reading is supported
        // over the other — so nothing is claimed. This is the designed outcome, and it is
        // pinned here because the obvious "fix" for it is a cue on 行く alone, which is
        // exactly the bug `particle_senses.rs` was written after.
        let station = sentence(&["駅", "に", "行く"]);
        let reading = senses(&station, 1, &categories).expect("に has senses");
        assert_eq!(reading.favoured, None);
    }

    #[test]
    fn counters_are_recognised_and_bare_numbers_are_not() {
        for counted in ["三日", "5人", "十時間", "何回"] {
            assert!(is_counted(counted), "{counted} should read as an amount");
        }
        for not in ["三", "5", "図書館", ""] {
            assert!(!is_counted(not), "{not} should not");
        }
    }
}
