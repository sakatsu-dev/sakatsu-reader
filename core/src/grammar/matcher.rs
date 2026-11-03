//! The pattern matcher: finds grammar patterns in a token sequence.
//!
//! **It matches tokens, never raw text.** A regex over the sentence would report a
//! には inside a single word as a grammar point, and a では that is really the start
//! of ではない — a match must begin and end on token boundaries, and here it does by
//! construction, because the smallest thing compared is a whole token
//! (`project notes` §5).
//!
//! Real Ichiran output shaped this design: Ichiran merges a great deal itself. には,
//! では and 何も each arrive as **one** token, and ことができる and はずです do too. So
//! most rules are one element long, and a matcher built to stitch particles together
//! from separate tokens would almost never fire.
//!
//! Matching is longest-first at each position, then non-overlapping: given には, the
//! には rule wins over に, and the は is not then reported separately — it is not a
//! second grammar point, it is part of the first.

use crate::model::Token;

use super::pack::{Element, Pack, Rule};

/// A pattern found in a token sequence.
///
/// `Deserialize` as well as `Serialize`: the interface sends these **back** when it asks for an
/// expression offer, so the answer is about the matches the reader was actually shown rather than a
/// fresh set recomputed in the command. Recomputing would be the cheaper-looking option and the one
/// that can silently disagree with what is on screen.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Match {
    /// The [`GrammarPattern`](super::pack::GrammarPattern) id; look the text up in
    /// the pack it came from.
    pub pattern_id: String,
    /// Index of the first token in the match.
    pub first_token: usize,
    /// Index one past the last token in the match.
    pub last_token_end: usize,
    /// Byte span in the source text, when every token in the match was grounded.
    pub start: Option<usize>,
    pub end: Option<usize>,
    /// What each element of the rule consumed, **as backtracking finally resolved it**.
    ///
    /// One entry per rule position, in rule order, so a caller can name `elements[0]` and
    /// `elements[2]` — the two endpoints of `AとかBとか` — instead of scanning the match for
    /// characters and hoping it finds the same thing the recogniser did.
    ///
    /// **Empty is possible and means "no element spans", never "the rule had none".** A match
    /// placed from a validated construction annotation
    /// ([`grammar::structural`](super::structural)) has no rule behind it, and a `Match`
    /// deserialized from an interface that predates this field arrives without one. Both are
    /// absences: a caller that needs an element span finds none and offers nothing, rather than
    /// falling back to a guess.
    #[serde(default)]
    pub elements: Vec<ElementSpan>,
}

impl Match {
    /// How many tokens the match covers.
    pub fn token_count(&self) -> usize {
        self.last_token_end - self.first_token
    }

    /// The span of one rule position, by index.
    pub fn element(&self, element: usize) -> Option<&ElementSpan> {
        self.elements.iter().find(|span| span.element == element)
    }
}

/// What one element of a rule consumed, on the path that actually matched.
///
/// # Why the consumed run and the ending are two different spans
///
/// `surface_ends: "とか"` consumes **whole tokens** — that is the `project notes` §5 guarantee, and
/// it is why `にはとり` is not a には. But the ending it matched on may begin *inside* the first
/// of those tokens: Ichiran hands back `映画とか` as one token, and the とか in it is four bytes
/// at the end of a word the rule had to consume entirely.
///
/// So both are recorded. [`ElementSpan::first_token`]`..`[`ElementSpan::last_token_end`] and
/// [`ElementSpan::start`]`..`[`ElementSpan::end`] are what the rule *consumed*;
/// [`ElementSpan::suffix_start`]`..`[`ElementSpan::suffix_end`] is where the **ending itself** is
/// written. A caller anchoring something to the とか wants the second; a caller highlighting what
/// the rule claimed wants the first. Conflating them would either under-claim the match or
/// over-claim the source.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ElementSpan {
    /// The element's position in the rule.
    pub element: usize,
    /// Index of the first token this element consumed.
    pub first_token: usize,
    /// Index one past the last token this element consumed. **Equal to `first_token` where a
    /// `{min: 0}` part consumed nothing**, which is recorded rather than omitted so that every
    /// later position keeps the index the rule gives it.
    pub last_token_end: usize,
    /// Byte span of the consumed tokens, on the same terms as [`Match::start`] — the first
    /// token's start and the last one's end, when both were grounded.
    pub start: Option<usize>,
    pub end: Option<usize>,
    /// The exact source span of the **ending** a `surface_ends` or `surface_ends_any` element
    /// matched on. `None` for every other kind of element, and `None` where the ending cannot be
    /// located in the source — an ungrounded token, a run that is not byte-contiguous, or a token
    /// whose surface is not what the source says.
    #[serde(default)]
    pub suffix_start: Option<usize>,
    #[serde(default)]
    pub suffix_end: Option<usize>,
}

impl ElementSpan {
    /// How many tokens this element consumed. Zero for a part that took none.
    pub fn token_count(&self) -> usize {
        self.last_token_end - self.first_token
    }

    /// Whether this element consumed nothing — a `{min: 0}` part that the rule did not need.
    pub fn is_empty(&self) -> bool {
        self.first_token == self.last_token_end
    }

    /// The ending's own span, when it could be located.
    pub fn suffix(&self) -> Option<(usize, usize)> {
        Some((self.suffix_start?, self.suffix_end?))
    }
}

/// Find every pattern in `tokens`, left to right.
///
/// At each position the longest rule wins; ties go to the pack's own order, so the
/// result is deterministic. A matched run is then skipped, so patterns never overlap.
pub fn find(tokens: &[Token], pack: &Pack) -> Vec<Match> {
    find_across(tokens, &[pack])
}

/// As [`find`], but over several packs at once, earliest pack winning a tie.
///
/// The curated pack and the imported JLPT pack both describe grammar, and a sentence
/// does not care which file a point was written in. Passing the curated pack first
/// makes it the authority where the two overlap — both packs have a `ので`, and the
/// hand-written one is the one with the better explanation.
pub fn find_across(tokens: &[Token], packs: &[&Pack]) -> Vec<Match> {
    let mut matches = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        match longest_at(tokens, index, packs) {
            Some(found) => {
                index = found.last_token_end;
                matches.push(found);
            }
            None => index += 1,
        }
    }

    matches
}

/// The longest rule matching at `index`, if any.
fn longest_at(tokens: &[Token], index: usize, packs: &[&Pack]) -> Option<Match> {
    let mut best: Option<Match> = None;
    let annotated: Vec<_> = crate::construction::resolved(&tokens[index])
        .into_iter()
        .filter(|annotation| annotation.grammar == crate::model::ConstructionGrammar::Sentence)
        .map(|annotation| annotation.grammar_pattern)
        .collect();

    for (pattern, rule) in packs.iter().flat_map(|pack| pack.matchable()) {
        // Composition already established this exact construction. `grammar::structural` places
        // it from the validated annotation instead of recognizing the same spelling again.
        if annotated.iter().any(|id| id == &pattern.id) {
            continue;
        }
        let Some((end, elements)) = rule_end(tokens, index, rule) else {
            continue;
        };
        // Strictly greater, so an earlier pack entry keeps a tie.
        // (Spelt out rather than `is_none_or`, which needs Rust 1.82 — the
        // workspace's MSRV is 1.80.)
        let beats_best = match &best {
            Some(current) => end - index > current.token_count(),
            None => true,
        };
        if beats_best {
            best = Some(Match {
                pattern_id: pattern.id.clone(),
                first_token: index,
                last_token_end: end,
                start: tokens[index].start,
                end: tokens[end - 1].end,
                elements,
            });
        }
    }

    best
}

/// Where a rule matching at `index` ends, or `None` if it does not match.
///
/// Returns the end **and** what each element consumed, so a caller can address a rule position
/// rather than search the matched run for the characters it expects to find there.
fn rule_end(tokens: &[Token], index: usize, rule: Rule<'_>) -> Option<(usize, Vec<ElementSpan>)> {
    match rule {
        Rule::Tokens(elements) => matches_at(tokens, index, elements, 0),
        // A text rule is one element by construction: the run it names.
        Rule::Text(text) => {
            let end = text_run_end(tokens, index, text)?;
            Some((end, vec![span_for(tokens, index, end, 0, None)]))
        }
    }
}

/// Match a sequence of parts starting at `index`, returning where it ends and what each part took.
///
/// Parts no longer consume exactly one token each — a part may repeat, and a `text`
/// part consumes however many tokens spell it — so this backtracks. Repetition is
/// greedy first, then gives tokens back one at a time until the rest of the rule fits,
/// which is what lets `[provisional] []{0,6} "ものを"` find the ものを that follows a
/// ば-form several words later without swallowing the whole sentence.
///
/// **The spans come back with the answer, so they describe the path that succeeded.** A part's
/// consumption is recorded only once the *rest of the rule* has matched after it, which is the
/// only moment the greedy count is known to be the final one. Recording as it went would report
/// the first count tried rather than the one that held.
fn matches_at(
    tokens: &[Token],
    index: usize,
    parts: &[Element],
    element: usize,
) -> Option<(usize, Vec<ElementSpan>)> {
    let Some((first, rest)) = parts.split_first() else {
        return Some((index, Vec::new()));
    };
    // Position belongs to where the part starts, so it is settled before anything is consumed.
    if let Some(wanted) = first.sentence_initial {
        if super::pack::opens_sentence(tokens, index) != wanted {
            return None;
        }
    }
    if let Some(wanted) = first.follows_word {
        if super::pack::follows_word(tokens, index) != wanted {
            return None;
        }
    }
    if index > 0 && first.not_after_any.contains(&tokens[index - 1].surface) {
        return None;
    }

    // A run part decides its own length; the repetition range does not apply. Any other
    // constraint on the same part describes the token the run ends on — that is what
    // separates the 向け of 子供向け, which does not inflect, from the 向け of 向けた,
    // which does.
    if first.text.is_some() || first.surface_ends.is_some() || !first.surface_ends_any.is_empty() {
        let (end, ending) = match (&first.text, &first.surface_ends) {
            (Some(text), _) => (text_run_end(tokens, index, text)?, None),
            (None, Some(suffix)) => {
                let run = suffix_run_end(tokens, index, suffix)?;
                (run.end, Some(run))
            }
            // Any of the alternatives, longest run first — same preference the matcher
            // applies everywhere else, so 呼んでください is reported whole rather than
            // whichever spelling happened to be listed first. `max_by_key` keeps the last
            // of a tie exactly as the `max()` it replaces did.
            (None, None) => {
                let run = first
                    .surface_ends_any
                    .iter()
                    .filter_map(|suffix| suffix_run_end(tokens, index, suffix))
                    .max_by_key(|run| run.end)?;
                (run.end, Some(run))
            }
        };
        if !first.matches_apart_from_spelling(&tokens[end - 1]) {
            return None;
        }
        let (finish, mut spans) = matches_at(tokens, end, rest, element + 1)?;
        spans.insert(0, span_for(tokens, index, end, element, ending));
        return Some((finish, spans));
    }

    let (min, max) = first.repeat();
    let mut taken = 0;
    while taken < max && index + taken < tokens.len() && first.matches(&tokens[index + taken]) {
        taken += 1;
    }

    loop {
        if taken < min {
            return None;
        }
        if let Some((finish, mut spans)) = matches_at(tokens, index + taken, rest, element + 1) {
            spans.insert(0, span_for(tokens, index, index + taken, element, None));
            return Some((finish, spans));
        }
        if taken == 0 {
            return None;
        }
        taken -= 1;
    }
}

/// Where an ending was found, and where it sits inside the run that was consumed to reach it.
#[derive(Debug, Clone, Copy)]
struct SuffixRun {
    /// Index one past the last consumed token.
    end: usize,
    /// Byte offset of the ending within the consumed tokens' concatenated surfaces.
    at: usize,
    /// The ending's length in bytes.
    len: usize,
}

/// Describe what one element consumed.
fn span_for(
    tokens: &[Token],
    first_token: usize,
    last_token_end: usize,
    element: usize,
    ending: Option<SuffixRun>,
) -> ElementSpan {
    // The same convention as `Match`: the outer bounds of the consumed run, and nothing at all
    // for a part that consumed nothing.
    let (start, end) = if first_token < last_token_end {
        (
            tokens.get(first_token).and_then(|token| token.start),
            tokens.get(last_token_end - 1).and_then(|token| token.end),
        )
    } else {
        (None, None)
    };
    let (suffix_start, suffix_end) = ending
        .and_then(|run| suffix_span(tokens, first_token, run))
        .map_or((None, None), |(at, to)| (Some(at), Some(to)));
    ElementSpan {
        element,
        first_token,
        last_token_end,
        start,
        end,
        suffix_start,
        suffix_end,
    }
}

/// Where the ending itself is written in the source, when that can be established.
///
/// **Every step here can fail, and each failure is a reason not to claim a span.** The ending may
/// begin inside a fused token, so this walks the consumed surfaces to find which token contains
/// it and offsets into that token's source. That offset is only meaningful if the token's surface
/// *is* its source text, so a token whose span length disagrees with its surface yields nothing
/// rather than an offset into the wrong place. The remaining tokens must be byte-contiguous, or
/// the ending is not one span. And the result must be exactly as long as the ending.
fn suffix_span(tokens: &[Token], first_token: usize, run: SuffixRun) -> Option<(usize, usize)> {
    let consumed = tokens.get(first_token..run.end)?;
    let mut offset = 0usize;
    for (position, token) in consumed.iter().enumerate() {
        let next = offset + token.surface.len();
        if run.at >= next {
            offset = next;
            continue;
        }
        let (token_start, token_end) = (token.start?, token.end?);
        // The surface must be what the source says, or an offset into it means nothing.
        if token_end.checked_sub(token_start)? != token.surface.len() {
            return None;
        }
        let start = token_start.checked_add(run.at - offset)?;
        // From here to the end of the run, the tokens must tile the source without gaps.
        let mut at = token_end;
        for following in &consumed[position + 1..] {
            if following.start? != at {
                return None;
            }
            at = following.end?;
        }
        if at.checked_sub(start)? != run.len {
            return None;
        }
        return Some((start, at));
    }
    None
}

/// The end of the run of whole tokens starting at `index` whose surfaces concatenate
/// to exactly `text`, if there is one.
///
/// Accumulating and comparing for equality is what keeps this honest: the run must
/// finish exactly on a token boundary. `にはとり` is one token, so starting there the
/// accumulation jumps straight past `には` and the rule correctly does not fire —
/// the same guarantee the element path gets for free, and the reason this is not a
/// substring search over the sentence.
/// The end of the shortest run of whole tokens from `index` whose surfaces *end with*
/// `suffix`.
///
/// The two analyzers need this to be one rule rather than two. `ません` arrives from
/// Lindera as `ませ` + `ん`, where the run spells it exactly, and from Ichiran inside
/// `知りません`, where one token swallowed it. Ending-with is true of both; equalling is
/// true of only the first.
///
/// Shortest, so the match claims no more of the sentence than it has to, and bounded,
/// so a rule cannot wander off looking for its ending.
fn suffix_run_end(tokens: &[Token], index: usize, suffix: &str) -> Option<SuffixRun> {
    const MOST: usize = 4;

    let mut run = String::new();
    let first_len = tokens.get(index)?.surface.len();
    for (offset, token) in tokens.get(index..)?.iter().take(MOST).enumerate() {
        run.push_str(&token.surface);
        if !run.ends_with(suffix) {
            continue;
        }
        // The first token must contribute to the suffix, or the run is starting further
        // left than it needs to and swallowing words that are no part of the pattern.
        //
        // Without this, `surface_ends: ください` matches all of 吸わ/ない/で/ください by
        // beginning at 吸わ — four tokens where one would do — and then outranks
        // `ないでください`, which is the better reading, purely by being longer.
        //
        // The suffix is allowed to begin *inside* that first token. That is the stated
        // meaning of `surface_ends`, and is how a fused 出席しない followed by こと/が/ある
        // can satisfy `ないことがある`. The old calculation accidentally asked whether
        // the final token contributed, rejecting every multi-token suffix with a fused
        // head even though its first token plainly supplied ない.
        let suffix_start = run.len() - suffix.len();
        let contributed = suffix_start < first_len;
        if contributed {
            return Some(SuffixRun {
                end: index + offset + 1,
                at: suffix_start,
                len: suffix.len(),
            });
        }
        return None;
    }
    None
}

fn text_run_end(tokens: &[Token], index: usize, text: &str) -> Option<usize> {
    let mut run = String::new();
    for (offset, token) in tokens.get(index..)?.iter().enumerate() {
        run.push_str(&token.surface);
        // Overshot: no longer run can come back to equality, so stop rather than
        // walking the rest of the sentence.
        if run.len() > text.len() {
            return None;
        }
        if run == text {
            return Some(index + offset + 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::pack::{self, Pack};
    use crate::model::{Conjugation, Prop};

    fn token(surface: &str) -> Token {
        Token {
            surface: surface.into(),
            dict_form: surface.into(),
            ..Default::default()
        }
    }

    fn ids(matches: &[Match]) -> Vec<&str> {
        matches.iter().map(|m| m.pattern_id.as_str()).collect()
    }

    #[test]
    fn a_particle_is_found_only_as_a_whole_token() {
        let pack = pack::core();

        // 私 / は / 学生 — は is its own token, so it is a grammar point.
        let split = [token("私"), token("は"), token("学生")];
        assert_eq!(ids(&find(&split, pack)), ["particle-wa"]);

        // The false hit project notes §5 warns about: a single word that *contains* には.
        // A regex would report a grammar point here; matching whole tokens cannot.
        let inside = [token("にはとり"), token("が")];
        assert_eq!(ids(&find(&inside, pack)), ["particle-ga"]);

        // Likewise ではない as one token is not a では.
        let dewanai = [token("ではない")];
        assert!(find(&dewanai, pack).is_empty());
    }

    #[test]
    fn the_longest_rule_wins_and_matches_do_not_overlap() {
        let pack = pack::core();
        // には is one token in real output, and beats に + は as a reading of it.
        let merged = [token("彼"), token("には"), token("時間")];
        assert_eq!(ids(&find(&merged, pack)), ["particle-niwa"]);

        // Two separate particles are two separate matches.
        let two = [token("本"), token("を"), token("私"), token("に")];
        assert_eq!(ids(&find(&two, pack)), ["particle-o", "particle-ni"]);
    }

    #[test]
    fn a_multi_token_rule_matches_on_conjugation_not_spelling() {
        let pack = pack::core();
        let ikanakereba = Token {
            surface: "行かなければ".into(),
            dict_form: "行く".into(),
            conjugation: Some(Conjugation {
                lemma: "行く".into(),
                reading: "いく".into(),
                props: vec![Prop {
                    form: "Provisional (~eba)".into(),
                    neg: true,
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };
        let tokens = [ikanakereba, token("ならない")];

        let found = find(&tokens, pack);
        assert_eq!(ids(&found), ["nakereba-naranai"]);
        assert_eq!(found[0].token_count(), 2, "the rule spans both tokens");

        // Without the negative, the rule must not fire.
        let mut positive = tokens.clone();
        positive[0]
            .conjugation
            .as_mut()
            .unwrap()
            .props
            .iter_mut()
            .for_each(|prop| prop.neg = false);
        assert!(find(&positive, pack).is_empty());
    }

    /// A token that spells its own source, starting at `at`.
    fn placed(surface: &str, at: usize) -> Token {
        Token {
            surface: surface.into(),
            dict_form: surface.into(),
            start: Some(at),
            end: Some(at + surface.len()),
            ..Default::default()
        }
    }

    /// Lay a sentence out as contiguous tokens, which is what the source actually is.
    fn laid_out(surfaces: &[&str]) -> Vec<Token> {
        let mut at = 0;
        surfaces
            .iter()
            .map(|surface| {
                let token = placed(surface, at);
                at += surface.len();
                token
            })
            .collect()
    }

    /// A one-pattern pack, so a test can state the rule it is about.
    fn rule_pack(id: &str, elements: &str) -> Pack {
        Pack::from_json(&format!(
            r#"{{"name":"t","patterns":[{{"id":"{id}","pattern":"p","title":"t",
               "explanation":"e","match":{elements}}}]}}"#
        ))
        .expect("the fixture pack parses")
    }

    /// **The endpoints of an enumerative list are rule positions, not a character scan.**
    ///
    /// `[{surface_ends とか}, {0,6}, {surface_ends とか}]` over 映画 と か 買い物 と か. Lindera
    /// splits とか into two tokens, so each endpoint is a *token run* rather than a token, and the
    /// middle wildcard takes the noun between them. Element 0 and element 2 are addressable;
    /// before this, the match knew only which tokens it covered in total.
    ///
    /// Note where the match **starts**: at と, not at 映画. `surface_ends` takes the shortest run
    /// that ends with the value and requires its first token to contribute, so the listed item is
    /// not swallowed into the endpoint. That was already true — it is asserted here because the
    /// element spans are what make it visible, and because an endpoint that claimed 映画 would
    /// hand the reader an offer over the wrong text.
    #[test]
    fn each_rule_position_reports_what_it_consumed() {
        let pack = rule_pack(
            "toka",
            r#"[{"surface_ends":"とか"},{"min":0,"max":6},{"surface_ends":"とか"}]"#,
        );
        let tokens = laid_out(&["映画", "と", "か", "買い物", "と", "か"]);

        let found = find(&tokens, &pack);
        assert_eq!(ids(&found), ["toka"]);
        let hit = &found[0];
        assert_eq!(hit.elements.len(), 3, "one span per rule position");
        assert_eq!(
            (hit.first_token, hit.last_token_end),
            (1, 6),
            "the list item before the first とか is no part of the pattern"
        );

        // Endpoint one: と か — two tokens spelling とか between them, and here the ending is
        // the whole of the run.
        let first = hit.element(0).expect("element 0");
        assert_eq!((first.first_token, first.last_token_end), (1, 3));
        assert_eq!((first.start, first.end), (Some(6), Some(12)));
        assert_eq!(first.suffix(), Some((6, 12)));

        // The wildcard took exactly the noun between the two endpoints.
        let middle = hit.element(1).expect("element 1");
        assert_eq!((middle.first_token, middle.last_token_end), (3, 4));
        assert_eq!((middle.start, middle.end), (Some(12), Some(21)));
        assert!(middle.suffix().is_none(), "a gap has no ending of its own");

        // Endpoint two: と か again, further along.
        let second = hit.element(2).expect("element 2");
        assert_eq!((second.first_token, second.last_token_end), (4, 6));
        assert_eq!(second.suffix(), Some((21, 27)));

        // **A position the rule does not have is not answered with one it does.** Falling back
        // to whatever span is nearest would hand a caller asking about the second endpoint the
        // first one's text.
        assert!(hit.element(3).is_none());
        assert!(hit.element(7).is_none());

        // The two endpoints are disjoint, the noun sits between them, and **neither endpoint
        // claims the parent's span** — which is what lets an occurrence be offered without
        // overwriting the construction it belongs to.
        assert!(first.suffix().unwrap().1 <= middle.start.unwrap());
        assert!(middle.end.unwrap() <= second.suffix().unwrap().0);
        assert_ne!(
            first.suffix().unwrap(),
            (hit.start.unwrap(), hit.end.unwrap())
        );
        assert_ne!(
            second.suffix().unwrap(),
            (hit.start.unwrap(), hit.end.unwrap())
        );
    }

    /// **The consumed run and the ending part company when a token is fused.**
    ///
    /// Ichiran hands back 映画とか whole. The rule must consume all of it — matching the four
    /// bytes alone is the thing `project notes` §5 forbids — while the ending is still only those
    /// four bytes of source.
    #[test]
    fn a_fused_token_separates_what_was_consumed_from_where_the_ending_is() {
        let pack = rule_pack(
            "toka",
            r#"[{"surface_ends":"とか"},{"min":0,"max":6},{"surface_ends":"とか"}]"#,
        );
        let tokens = laid_out(&["映画とか", "買い物", "とか"]);

        let found = find(&tokens, &pack);
        let hit = &found[0];
        let first = hit.element(0).expect("element 0");
        assert_eq!(
            (first.first_token, first.last_token_end),
            (0, 1),
            "the whole fused token is consumed"
        );
        assert_eq!((first.start, first.end), (Some(0), Some(12)));
        assert_eq!(
            first.suffix(),
            Some((6, 12)),
            "but the ending is written in the last six bytes of it"
        );
    }

    /// **A repeated element reports the count backtracking settled on, not the greedy one.**
    #[test]
    fn a_repeated_element_reports_the_count_that_finally_held() {
        let pack = rule_pack(
            "gap",
            r#"[{"surface":"から"},{"min":0,"max":6},{"surface":"まで"}]"#,
        );
        let tokens = laid_out(&["から", "あ", "い", "う", "まで"]);

        let found = find(&tokens, &pack);
        assert_eq!(ids(&found), ["gap"]);
        let middle = found[0].element(1).expect("element 1");
        // Greedy first would have taken あ い う まで — four — and then failed. Three is what
        // let the rest of the rule fit, and three is what is reported.
        assert_eq!(middle.token_count(), 3);
        assert_eq!((middle.first_token, middle.last_token_end), (1, 4));
        assert_eq!((middle.start, middle.end), (Some(6), Some(15)));
    }

    /// A wildcard at its `min` of zero consumes nothing — and says so, keeping the positions
    /// after it addressable.
    #[test]
    fn a_wildcard_at_its_minimum_reports_an_empty_span() {
        let pack = rule_pack(
            "gap",
            r#"[{"surface":"から"},{"min":0,"max":6},{"surface":"まで"}]"#,
        );
        let tokens = laid_out(&["から", "まで"]);

        let found = find(&tokens, &pack);
        let hit = &found[0];
        let middle = hit.element(1).expect("element 1");
        assert!(middle.is_empty());
        assert_eq!(middle.token_count(), 0);
        assert_eq!((middle.start, middle.end), (None, None));
        // The position after it is still position 2.
        assert_eq!(hit.element(2).expect("element 2").first_token, 1);
    }

    /// A wildcard at its `max` stops there rather than running on.
    #[test]
    fn a_wildcard_at_its_maximum_stops_there() {
        let pack = rule_pack(
            "gap",
            r#"[{"surface":"から"},{"min":0,"max":2},{"surface":"まで"}]"#,
        );
        // Three tokens between the endpoints is one more than the rule allows.
        let too_far = laid_out(&["から", "あ", "い", "う", "まで"]);
        assert!(find(&too_far, &pack).is_empty());

        let exactly = laid_out(&["から", "あ", "い", "まで"]);
        let found = find(&exactly, &pack);
        assert_eq!(found[0].element(1).expect("element 1").token_count(), 2);
    }

    /// **Longest-match and tie behaviour are unchanged**, which is the thing element spans were
    /// not allowed to cost.
    #[test]
    fn carrying_element_spans_does_not_change_which_rule_wins() {
        let pack = pack::core();

        // には beats に, and the は is not reported separately.
        let niwa = [token("私"), token("には"), token("ある")];
        assert_eq!(ids(&find(&niwa, pack)), ["particle-niwa"]);

        // A tie goes to the earlier pack entry, and the earlier pack.
        let first = rule_pack("first", r#"[{"surface":"か"}]"#);
        let second = rule_pack("second", r#"[{"surface":"か"}]"#);
        let tokens = laid_out(&["か"]);
        assert_eq!(ids(&find_across(&tokens, &[&first, &second])), ["first"]);
        assert_eq!(ids(&find_across(&tokens, &[&second, &first])), ["second"]);

        // A longer rule still wins over a shorter one at the same position, with the spans of
        // the winner and nobody else.
        let short = rule_pack("short", r#"[{"surface":"から"}]"#);
        let long = rule_pack(
            "long",
            r#"[{"surface":"から"},{"min":0,"max":6},{"surface":"まで"}]"#,
        );
        let tokens = laid_out(&["から", "あ", "まで"]);
        let found = find_across(&tokens, &[&short, &long]);
        assert_eq!(ids(&found), ["long"]);
        assert_eq!(found[0].elements.len(), 3);
    }

    /// **An ending that cannot be located in the source yields no span rather than a guess.**
    #[test]
    fn an_ending_that_cannot_be_placed_reports_nothing() {
        let pack = rule_pack("toka", r#"[{"surface_ends":"とか"}]"#);

        // Ungrounded tokens: the match still fires, and the ending has no source to point at.
        let floating = [token("映画"), token("と"), token("か")];
        let found = find(&floating, &pack);
        assert_eq!(ids(&found), ["toka"]);
        assert!(found[0].element(0).expect("element 0").suffix().is_none());

        // A gap between the tokens means the ending is not one contiguous span.
        let mut split = laid_out(&["映画", "と", "か"]);
        split[2].start = Some(split[2].start.unwrap() + 1);
        split[2].end = Some(split[2].end.unwrap() + 1);
        let found = find(&split, &pack);
        assert_eq!(ids(&found), ["toka"]);
        assert!(found[0].element(0).expect("element 0").suffix().is_none());

        // A token whose span disagrees with its surface cannot be offset into.
        let mut lying = laid_out(&["映画とか"]);
        lying[0].end = Some(lying[0].end.unwrap() + 3);
        let found = find(&lying, &pack);
        assert_eq!(ids(&found), ["toka"]);
        assert!(found[0].element(0).expect("element 0").suffix().is_none());
    }

    /// **Two lies that cancel out still cannot be offset into.**
    ///
    /// The length of the located ending is checked at the end, and for a single token that alone
    /// is enough: a token whose span is *n* bytes off puts the ending *n* bytes off, and the
    /// length check catches it. Across a run it does not. A first token three bytes too long
    /// followed by one three bytes too short leaves the total exactly right while the offset
    /// *into the first token* is meaningless — so the surface and the span are compared per
    /// token, before any of the arithmetic.
    #[test]
    fn a_run_whose_lies_cancel_out_is_still_refused() {
        let pack = rule_pack("toka", r#"[{"surface_ends":"とかです"}]"#);

        let mut tokens = laid_out(&["映画とか", "です"]);
        assert_eq!(
            tokens[0].end.unwrap() - tokens[0].start.unwrap(),
            tokens[0].surface.len(),
            "the honest layout first, so the test is about the change it makes"
        );
        let honest = find(&tokens, &pack);
        assert_eq!(
            honest[0].element(0).expect("element 0").suffix(),
            Some((6, 18)),
            "honestly placed, the ending is found"
        );

        // Three bytes too long, then three bytes too short: contiguous, same total, wrong inside.
        tokens[0].end = Some(tokens[0].end.unwrap() + 3);
        tokens[1].start = Some(tokens[1].start.unwrap() + 3);
        let found = find(&tokens, &pack);
        assert_eq!(ids(&found), ["toka"]);
        assert!(
            found[0].element(0).expect("element 0").suffix().is_none(),
            "the totals agree and the offset does not; a span nobody can verify is not drawn"
        );
    }

    /// A `match_text` rule is one element, and reports itself as one.
    #[test]
    fn a_text_rule_reports_a_single_element() {
        let pack = Pack::from_json(
            r#"{"name":"t","patterns":[{"id":"kotokara","pattern":"p","title":"t",
               "explanation":"e","match_text":["ことから"]}]}"#,
        )
        .expect("the fixture pack parses");
        let tokens = laid_out(&["こと", "から"]);

        let found = find(&tokens, &pack);
        assert_eq!(ids(&found), ["kotokara"]);
        let only = found[0].element(0).expect("element 0");
        assert_eq!((only.first_token, only.last_token_end), (0, 2));
        assert_eq!((only.start, only.end), (Some(0), Some(12)));
        assert!(
            only.suffix().is_none(),
            "a text rule spells its run exactly; it has no ending inside a token"
        );
    }

    #[test]
    fn a_suffix_run_can_begin_at_the_end_of_its_first_fused_token() {
        let fused = [
            token("出席しない"),
            token("こと"),
            token("が"),
            token("ある"),
        ];
        let run =
            suffix_run_end(&fused, 0, "ないことがある").expect("the fused head supplies ない");
        assert_eq!(run.end, 4);
        // The ending begins inside 出席しない, six bytes in — which is the whole reason the
        // consumed run and the ending are two different spans.
        assert_eq!(run.at, "出席し".len());
        assert_eq!(run.len, "ないことがある".len());

        // Starting before the suffix contributes nothing and must still be refused.
        let too_early = [token("吸わ"), token("ない"), token("で"), token("ください")];
        assert!(suffix_run_end(&too_early, 0, "ください").is_none());
    }

    #[test]
    fn a_match_carries_the_byte_span_of_the_tokens_it_covers() {
        let pack = pack::core();
        let mut wa = token("は");
        wa.start = Some(3);
        wa.end = Some(6);
        let tokens = [token("私"), wa];

        let found = find(&tokens, pack);
        assert_eq!(found[0].start, Some(3));
        assert_eq!(found[0].end, Some(6));
    }

    #[test]
    fn a_pattern_with_no_match_rule_is_never_matched_by_surface() {
        // The 〜て auxiliaries are found structurally, by the compound detector. A
        // token that merely reads てくる must not be reported by this matcher.
        let pack = pack::core();
        assert!(find(&[token("てくる")], pack).is_empty());
        assert!(pack.get("te-kuru").unwrap().match_tokens.is_none());
    }

    #[test]
    fn specific_obligation_and_whether_or_not_rules_are_not_shadowed() {
        let provisional_negative = |surface: &str| Token {
            surface: surface.into(),
            dict_form: "帰る".into(),
            conjugation: Some(Conjugation {
                lemma: "帰る".into(),
                reading: "かえる".into(),
                props: vec![Prop {
                    form: "Provisional (~eba)".into(),
                    neg: true,
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };
        let packs = pack::all();

        let neba = [provisional_negative("帰らねば"), token("ならない")];
        assert_eq!(
            ids(&find_across(&neba, &packs)),
            ["hanabira-n2-neba-naranai"]
        );

        let nakereba = [provisional_negative("帰らなければ"), token("ならない")];
        assert_eq!(ids(&find_across(&nakereba, &packs)), ["nakereba-naranai"]);

        // The exact negative alternative is more informative than the general
        // A-or-B construction, so its rule deliberately includes the supplied
        // predicate and wins by one token.
        let whether_or_not = [
            token("難しい"),
            token("だろう"),
            token("と"),
            token("なかろう"),
            token("と"),
        ];
        assert_eq!(
            ids(&find_across(&whether_or_not, &packs)),
            ["hanabira-n1-darou-to-nakarou-to"]
        );
    }

    #[test]
    fn an_empty_sequence_and_an_empty_pack_find_nothing() {
        assert!(find(&[], pack::core()).is_empty());
        let empty = Pack::from_json(r#"{ "name": "empty", "patterns": [] }"#).expect("parse");
        assert!(find(&[token("は")], &empty).is_empty());
    }
}
