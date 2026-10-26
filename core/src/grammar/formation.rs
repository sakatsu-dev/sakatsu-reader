//! The formation generator: *how this particular word builds this form*.
//!
//! Works on the **kana reading**, because kana is where Japanese conjugation is
//! phonologically regular. Working on kanji would mean guessing at okurigana, so
//! this never fabricates a spelling (`project notes` §5).
//!
//! It returns `None` rather than a guess whenever the combination is not one it can
//! build honestly — an absent explanation costs the learner nothing, a wrong one
//! teaches them something false.

use super::form::Form;
use crate::kana::to_hiragana;

/// How a form is built from a dictionary word.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Formation {
    /// The kana the form is built from, e.g. `"いう"`.
    pub from: String,
    /// The change applied, e.g. `"う → わ, then add せる"`.
    pub change: String,
    /// The resulting kana, e.g. `"いわせる"`.
    pub result: String,
}

/// A documented contraction of a form [`formation`] has already built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contracted {
    pub family: crate::model::ContractionFamily,
    /// `ら → ん` before `な` was applied.
    pub nasal: bool,
    /// The contracted ending the result closes on: `とる`, `どる`, `なきゃ`, `なけりゃ`.
    pub tail: &'static str,
    /// `from` is the **uncontracted** form (`まちがっておる`, `おこらなければ`); `result` the
    /// contraction (`まちがっとる`, `おこんなきゃ`).
    pub formation: Formation,
}

/// **The one table of contractions**, applied to a form already built by [`formation`].
///
/// Shared by the repair that recognises a contracted word (`lexical::contraction`) and the ladder
/// that explains one (`derivation`), so what is accepted and what is shown cannot drift apart.
///
/// - after the te-form (`Conjunctive`): `～て + おる → ～とる`, `～で + おる → ～どる`;
/// - after the negative provisional: `～なければ → ～なきゃ | ～なけりゃ`, and for a **godan** base
///   ending in ら — the r-row, since a godan base is the a-row of its final kana — also
///   `ら → ん` before `な`: 怒らなきゃ → 怒んなきゃ. An ichidan base has no ら to nasalise.
///
/// Anything else contracts to nothing: this names the rewrites it documents and no others.
pub fn contractions(built: &str, form: Form, neg: bool, pos: &str) -> Vec<Contracted> {
    use crate::model::ContractionFamily;
    let mut found = Vec::new();
    match (form, neg) {
        (Form::Conjunctive, false) => {
            for (te, tail) in [("て", "とる"), ("で", "どる")] {
                if let Some(stem) = built.strip_suffix(te) {
                    found.push(Contracted {
                        family: ContractionFamily::TeOru,
                        nasal: false,
                        tail,
                        formation: Formation {
                            from: format!("{built}おる"),
                            change: format!("{te}おる → {tail}"),
                            result: format!("{stem}{tail}"),
                        },
                    });
                }
            }
        }
        (Form::Provisional, true) => {
            let Some(base) = built.strip_suffix("なければ") else {
                return found;
            };
            let godan = matches!(Class::from_pos(pos), Some(Class::Godan { .. }));
            for tail in ["なきゃ", "なけりゃ"] {
                found.push(Contracted {
                    family: ContractionFamily::NegativeConditional,
                    nasal: false,
                    tail,
                    formation: Formation {
                        from: built.to_string(),
                        change: format!("なければ → {tail}"),
                        result: format!("{base}{tail}"),
                    },
                });
                if let Some(before) = base.strip_suffix('ら').filter(|_| godan) {
                    found.push(Contracted {
                        family: ContractionFamily::NegativeConditional,
                        nasal: true,
                        tail,
                        formation: Formation {
                            from: built.to_string(),
                            change: format!("なければ → {tail}, ら → ん"),
                            result: format!("{before}ん{tail}"),
                        },
                    });
                }
            }
        }
        _ => {}
    }
    found
}

/// Which conjugation class a POS code names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// 五段 — the final kana shifts row. `iku` marks 行く, whose te-form is irregular.
    Godan { iku: bool },
    /// 一段 — drop る and add the suffix.
    Ichidan,
    /// 来る.
    Kuru,
    /// する and the noun+する verbs.
    Suru,
    /// い-adjective.
    IAdjective,
}

impl Class {
    /// Ichiran's POS codes: `v5u`/`v5r`/… godan, `v5k-s` = 行く, `v1`/`v1-s`
    /// ichidan, `vk` = 来る, `vs*` = する, `adj-i`.
    fn from_pos(pos: &str) -> Option<Class> {
        let pos = pos.trim().to_ascii_lowercase();
        if pos == "v5k-s" {
            Some(Class::Godan { iku: true })
        } else if pos.starts_with("v5") {
            Some(Class::Godan { iku: false })
        } else if pos.starts_with("v1") {
            Some(Class::Ichidan)
        } else if pos == "vk" {
            Some(Class::Kuru)
        } else if pos.starts_with("vs") {
            Some(Class::Suru)
        } else if pos.starts_with("adj-i") {
            Some(Class::IAdjective)
        } else {
            None
        }
    }
}

/// The five vowel rows a godan verb's final kana moves through.
#[derive(Debug, Clone, Copy)]
enum Row {
    A,
    I,
    E,
    O,
}

/// `(dictionary, -a, -i, -e, -o)`.
///
/// The trap is う → **わ**, not あ: 言う's negative stem is いわ. Getting this
/// wrong produces いあない, which is not a word.
const GODAN_ROWS: [(char, char, char, char, char); 9] = [
    ('う', 'わ', 'い', 'え', 'お'),
    ('く', 'か', 'き', 'け', 'こ'),
    ('ぐ', 'が', 'ぎ', 'げ', 'ご'),
    ('す', 'さ', 'し', 'せ', 'そ'),
    ('つ', 'た', 'ち', 'て', 'と'),
    ('ぬ', 'な', 'に', 'ね', 'の'),
    ('ぶ', 'ば', 'び', 'べ', 'ぼ'),
    ('む', 'ま', 'み', 'め', 'も'),
    ('る', 'ら', 'り', 'れ', 'ろ'),
];

/// Euphonic (音便) te/ta endings, by the verb's final kana.
const EUPHONIC: [(char, &str, &str); 9] = [
    ('う', "って", "った"),
    ('つ', "って", "った"),
    ('る', "って", "った"),
    ('む', "んで", "んだ"),
    ('ぶ', "んで", "んだ"),
    ('ぬ', "んで", "んだ"),
    ('く', "いて", "いた"),
    ('ぐ', "いで", "いだ"),
    ('す', "して", "した"),
];

/// Build the formation for `form` applied to the word read `reading`, whose class
/// is named by the POS code `pos`.
///
/// `neg` and `fml` are the flags from the same property. They are only composed for
/// the plain past and non-past — the combinations a learner meets constantly. Any
/// other flagged combination returns `None` instead of describing the wrong word:
/// the formation for 食べなかった is not the formation for 食べた.
pub fn formation(reading: &str, pos: &str, form: Form, neg: bool, fml: bool) -> Option<Formation> {
    let from = to_hiragana(reading.trim());
    if from.is_empty() {
        return None;
    }
    let class = Class::from_pos(pos)?;

    match form {
        Form::NonPast | Form::Past => tense(&from, class, form, neg, fml),
        // 行かなければ / 食べなければ / 高くなければ. Modelled because the
        // 〜なければならない pattern is built on it, and a pattern whose own formation
        // cannot be shown is a gap a learner notices immediately.
        Form::Provisional if neg && !fml => match class {
            Class::IAdjective => adjective_form(&from, "くなければ"),
            _ => suffix_form(
                &from,
                class,
                Row::A,
                "なければ",
                "なければ",
                "こなければ",
                "しなければ",
            ),
        },
        // Every other form composes with neg/fml in ways this does not model yet.
        _ if neg || fml => None,
        Form::Causative => {
            suffix_form(&from, class, Row::A, "せる", "させる", "こさせる", "させる")
        }
        Form::Passive => suffix_form(&from, class, Row::A, "れる", "られる", "こられる", "される"),
        Form::CausativePassive => suffix_form(
            &from,
            class,
            Row::A,
            "せられる",
            "させられる",
            "こさせられる",
            "させられる",
        ),
        Form::Potential => suffix_form(&from, class, Row::E, "る", "られる", "こられる", "できる"),
        Form::Provisional => suffix_form(&from, class, Row::E, "ば", "れば", "くれば", "すれば"),
        Form::Volitional => suffix_form(&from, class, Row::O, "う", "よう", "こよう", "しよう"),
        Form::Imperative => suffix_form(&from, class, Row::E, "", "ろ", "こい", "しろ"),
        Form::Continuative => suffix_form(&from, class, Row::I, "", "", "き", "し"),
        Form::Conjunctive => euphonic_form(&from, class, Euphonic::Te, ""),
        Form::Conditional => euphonic_form(&from, class, Euphonic::Ta, "ら"),
        Form::Alternative => euphonic_form(&from, class, Euphonic::Ta, "り"),
        _ => None,
    }
}

/// Non-past and past, which are where `neg` and `fml` actually land.
fn tense(from: &str, class: Class, form: Form, neg: bool, fml: bool) -> Option<Formation> {
    match (form, neg, fml) {
        // The dictionary form itself — there is nothing to explain.
        (Form::NonPast, false, false) => None,

        // 食べます / 言います / きます / します — but an い-adjective just takes です,
        // with no stem change at all (情けない → 情けないです).
        (Form::NonPast, false, true) => match class {
            Class::IAdjective => Some(appended(from, "です")),
            _ => suffix_form(from, class, Row::I, "ます", "ます", "きます", "します"),
        },
        // 食べました / 情けなかったです
        (Form::Past, false, true) => match class {
            Class::IAdjective => adjective_form(from, "かったです"),
            _ => suffix_form(
                from,
                class,
                Row::I,
                "ました",
                "ました",
                "きました",
                "しました",
            ),
        },
        // 食べない / 言わない / こない / しない / 高くない
        (Form::NonPast, true, false) => match class {
            Class::IAdjective => adjective_form(from, "くない"),
            _ => suffix_form(from, class, Row::A, "ない", "ない", "こない", "しない"),
        },
        // 食べなかった / 言わなかった / 高くなかった
        (Form::Past, true, false) => match class {
            Class::IAdjective => adjective_form(from, "くなかった"),
            _ => suffix_form(
                from,
                class,
                Row::A,
                "なかった",
                "なかった",
                "こなかった",
                "しなかった",
            ),
        },
        // 食べた / 言った — euphonic for godan, plain for the rest.
        (Form::Past, false, false) => match class {
            Class::IAdjective => adjective_form(from, "かった"),
            _ => euphonic_form(from, class, Euphonic::Ta, ""),
        },
        // Negative formal (食べませんでした) and the rest are not modelled.
        _ => None,
    }
}

/// Row-shift (godan) or drop-る (ichidan) plus a suffix, with the irregulars given
/// outright because they are irregular — a rule that "derives" こさせる from くる
/// would be a fiction.
fn suffix_form(
    from: &str,
    class: Class,
    row: Row,
    godan_suffix: &str,
    ichidan_suffix: &str,
    kuru: &str,
    suru: &str,
) -> Option<Formation> {
    match class {
        Class::Godan { .. } => {
            let (stem, last) = split_last(from)?;
            let shifted = shift(last, row)?;
            let result = format!("{stem}{shifted}{godan_suffix}");
            let change = if godan_suffix.is_empty() {
                format!("{last} → {shifted}")
            } else {
                format!("{last} → {shifted}, then add {godan_suffix}")
            };
            Some(Formation {
                from: from.to_string(),
                change,
                result,
            })
        }
        Class::Ichidan => {
            let stem = from.strip_suffix('る')?;
            let change = if ichidan_suffix.is_empty() {
                "drop る".to_string()
            } else {
                format!("drop る, then add {ichidan_suffix}")
            };
            Some(Formation {
                from: from.to_string(),
                change,
                result: format!("{stem}{ichidan_suffix}"),
            })
        }
        Class::Kuru => Some(irregular(from, kuru)),
        Class::Suru => Some(irregular(from, suru)),
        Class::IAdjective => None,
    }
}

enum Euphonic {
    Te,
    Ta,
}

/// The te/ta forms, where godan verbs take a sound change rather than a row shift.
fn euphonic_form(from: &str, class: Class, which: Euphonic, tail: &str) -> Option<Formation> {
    let plain = match which {
        Euphonic::Te => "て",
        Euphonic::Ta => "た",
    };

    match class {
        Class::Godan { iku } => {
            let (stem, last) = split_last(from)?;
            // 行く is the classic exception: いいて would be wrong, it is いって.
            let ending = if iku {
                match which {
                    Euphonic::Te => "って",
                    Euphonic::Ta => "った",
                }
            } else {
                let found = EUPHONIC.iter().find(|(kana, ..)| *kana == last)?;
                match which {
                    Euphonic::Te => found.1,
                    Euphonic::Ta => found.2,
                }
            };
            Some(Formation {
                from: from.to_string(),
                change: format!("{last} → {ending}{tail}"),
                result: format!("{stem}{ending}{tail}"),
            })
        }
        Class::Ichidan => {
            let stem = from.strip_suffix('る')?;
            Some(Formation {
                from: from.to_string(),
                change: format!("drop る, then add {plain}{tail}"),
                result: format!("{stem}{plain}{tail}"),
            })
        }
        Class::Kuru => Some(irregular(from, &format!("き{plain}{tail}"))),
        Class::Suru => Some(irregular(from, &format!("し{plain}{tail}"))),
        Class::IAdjective => adjective_form(from, &format!("く{plain}{tail}")),
    }
}

/// い-adjectives replace the final い.
fn adjective_form(from: &str, suffix: &str) -> Option<Formation> {
    let stem = from.strip_suffix('い')?;
    Some(Formation {
        from: from.to_string(),
        change: format!("い → {suffix}"),
        result: format!("{stem}{suffix}"),
    })
}

/// A suffix that attaches with no change to the word at all.
fn appended(from: &str, suffix: &str) -> Formation {
    Formation {
        from: from.to_string(),
        change: format!("add {suffix}"),
        result: format!("{from}{suffix}"),
    }
}

fn irregular(from: &str, result: &str) -> Formation {
    Formation {
        from: from.to_string(),
        change: format!("irregular: {from} → {result}"),
        result: result.to_string(),
    }
}

fn split_last(text: &str) -> Option<(&str, char)> {
    let last = text.chars().next_back()?;
    Some((&text[..text.len() - last.len_utf8()], last))
}

fn shift(last: char, row: Row) -> Option<char> {
    GODAN_ROWS
        .iter()
        .find(|(dictionary, ..)| *dictionary == last)
        .map(|&(_, a, i, e, o)| match row {
            Row::A => a,
            Row::I => i,
            Row::E => e,
            Row::O => o,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(reading: &str, pos: &str, form: Form) -> String {
        formation(reading, pos, form, false, false)
            .unwrap_or_else(|| panic!("no formation for {reading} {pos} {form:?}"))
            .result
    }

    #[test]
    fn a_godan_verb_shifts_its_final_kana_and_u_goes_to_wa() {
        // The trap: the -a row of う is わ. いあせる is not a word.
        assert_eq!(built("いう", "v5u", Form::Causative), "いわせる");
        assert_eq!(built("いう", "v5u", Form::Passive), "いわれる");
        assert_eq!(built("のむ", "v5m", Form::Causative), "のませる");
        assert_eq!(built("のむ", "v5m", Form::Potential), "のめる");
        assert_eq!(built("いく", "v5k-s", Form::Volitional), "いこう");
    }

    #[test]
    fn an_ichidan_verb_drops_ru() {
        assert_eq!(built("たべる", "v1", Form::Causative), "たべさせる");
        assert_eq!(
            built("たべる", "v1", Form::CausativePassive),
            "たべさせられる"
        );
        assert_eq!(built("たべる", "v1", Form::Conjunctive), "たべて");

        let formation = formation("たべる", "v1", Form::Causative, false, false).unwrap();
        assert_eq!(formation.change, "drop る, then add させる");
        assert_eq!(formation.from, "たべる");
    }

    #[test]
    fn the_te_and_ta_forms_use_the_euphonic_endings() {
        assert_eq!(built("つたわる", "v5r", Form::Conjunctive), "つたわって");
        assert_eq!(built("のむ", "v5m", Form::Conjunctive), "のんで");
        assert_eq!(built("かく", "v5k", Form::Conjunctive), "かいて");
        assert_eq!(built("いそぐ", "v5g", Form::Conjunctive), "いそいで");
        assert_eq!(built("はなす", "v5s", Form::Conjunctive), "はなして");
        assert_eq!(built("まつ", "v5t", Form::Past), "まった");

        // 行く is irregular here: いいて would be wrong.
        assert_eq!(built("いく", "v5k-s", Form::Conjunctive), "いって");
        assert_eq!(built("いく", "v5k-s", Form::Past), "いった");
    }

    #[test]
    fn the_irregular_verbs_are_given_not_derived() {
        assert_eq!(built("くる", "vk", Form::Causative), "こさせる");
        assert_eq!(built("くる", "vk", Form::Conjunctive), "きて");
        assert_eq!(built("する", "vs-i", Form::Potential), "できる");
        assert_eq!(built("する", "vs-i", Form::Conjunctive), "して");

        // A katakana-stem reading is normalized before use (Ichiran's 【クる】).
        assert_eq!(built("クる", "vk", Form::Conjunctive), "きて");
    }

    #[test]
    fn the_formal_and_negative_tenses_are_composed() {
        assert_eq!(
            formation("くる", "vk", Form::NonPast, false, true)
                .unwrap()
                .result,
            "きます"
        );
        assert_eq!(
            formation("たべる", "v1", Form::Past, true, false)
                .unwrap()
                .result,
            "たべなかった"
        );
        assert_eq!(
            formation("いう", "v5u", Form::NonPast, true, false)
                .unwrap()
                .result,
            "いわない"
        );
        assert_eq!(
            formation("なさけない", "adj-i", Form::NonPast, false, true)
                .unwrap()
                .result,
            "なさけないです"
        );
    }

    #[test]
    fn the_negative_provisional_is_built_because_a_pattern_depends_on_it() {
        // The first half of 〜なければならない.
        let built = |reading: &str, pos: &str| {
            formation(reading, pos, Form::Provisional, true, false)
                .unwrap_or_else(|| panic!("no formation for {reading}"))
                .result
        };
        assert_eq!(built("いく", "v5k-s"), "いかなければ");
        assert_eq!(built("たべる", "v1"), "たべなければ");
        assert_eq!(built("くる", "vk"), "こなければ");
        assert_eq!(built("する", "vs-i"), "しなければ");
        assert_eq!(built("たかい", "adj-i"), "たかくなければ");

        // The plain provisional is unaffected.
        assert_eq!(
            formation("いく", "v5k-s", Form::Provisional, false, false)
                .unwrap()
                .result,
            "いけば"
        );
    }

    #[test]
    fn the_plain_dictionary_form_and_unmodelled_combinations_explain_nothing() {
        // Nothing to say about how 食べる builds 食べる.
        assert!(formation("たべる", "v1", Form::NonPast, false, false).is_none());
        // A negative causative is not modelled, and must not be described as a
        // plain causative — that would name the wrong word.
        assert!(formation("たべる", "v1", Form::Causative, true, false).is_none());
        // Negative formal past is not modelled either.
        assert!(formation("たべる", "v1", Form::Past, true, true).is_none());
        // An unknown class explains nothing rather than guessing.
        assert!(formation("きれい", "adj-na", Form::Past, false, false).is_none());
        assert!(formation("", "v1", Form::Past, false, false).is_none());
    }
}
