//! A glossary of the grammatical vocabulary the app itself uses.
//!
//! Two different kinds of jargon reach the reader, and they used to be explained — or not
//! explained — separately:
//!
//! - **Codes** on a dictionary sense: `n`, `conj`, `vi`, `v5m`. [`crate::dict::tags`] can
//!   expand these into a name, which tells you `conj` is short for "conjunction" and
//!   nothing more.
//! - **Words** the panels print in prose: causative, potential, predicate, te-form. These
//!   had no explanation anywhere.
//!
//! One table covers both, because a reader who wants to know what a conjunction *is* has
//! the same question either way. The ids of the grammatical entries are deliberately the
//! same strings the breakdown uses for its forms, so the grammar panel can look a term up
//! by id rather than by matching the text it printed.
//!
//! Nothing here is shown unless it is asked for. The explanations are written for someone
//! who half-remembers the word — short, concrete, and with a Japanese example wherever the
//! example does more work than another sentence would.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Serialize;

/// One entry: what it is called, what else to call it, and what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Term {
    /// Stable key. For grammatical forms this matches the breakdown's `form` field.
    pub id: &'static str,
    /// What to print as a heading.
    pub label: &'static str,
    /// Everything a search should match, lowercase. Includes the id and the label.
    pub aliases: &'static [&'static str],
    /// One or two sentences.
    pub detail: &'static str,
    /// Which of the table's five groups this belongs to.
    ///
    /// The table was already written in these groups and separated by comments; naming
    /// them makes the grouping data the interface can sort by, rather than an accident of
    /// the order entries happen to be listed in.
    pub kind: Kind,
}

/// The lossless, learner-facing shape shared by Reference and Glossary.
///
/// The compact table above stays pleasant to curate. This owned view separates the first
/// useful sentence from the supporting prose and adds only explicitly reviewed links; the
/// interface never has to infer a relationship from two English strings that happen to match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TermView {
    pub id: String,
    pub label: String,
    pub aliases: Vec<String>,
    pub summary: String,
    pub detail: Option<String>,
    pub examples: Vec<TermExample>,
    pub requires: Vec<String>,
    pub contrasts: Vec<String>,
    pub related: Vec<String>,
    pub kind: Kind,
}

/// One example whose two languages must remain paired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TermExample {
    pub japanese: String,
    pub english: String,
}

#[derive(Debug, Clone, Copy)]
struct Relations {
    id: &'static str,
    requires: &'static [&'static str],
    contrasts: &'static [&'static str],
    related: &'static [&'static str],
}

/// Small on purpose: these are pedagogical claims, not links synthesized from prose.
const RELATIONS: &[Relations] = &[
    Relations {
        id: "causative-passive",
        requires: &["causative", "passive"],
        contrasts: &[],
        related: &[],
    },
    Relations {
        id: "adj-i",
        requires: &[],
        contrasts: &["adj-na"],
        related: &[],
    },
    Relations {
        id: "adj-na",
        requires: &[],
        contrasts: &["adj-i"],
        related: &[],
    },
    Relations {
        id: "vt",
        requires: &[],
        contrasts: &["vi"],
        related: &["transitive-pair"],
    },
    Relations {
        id: "vi",
        requires: &[],
        contrasts: &["vt"],
        related: &["transitive-pair"],
    },
    Relations {
        id: "transitive-pair",
        requires: &[],
        contrasts: &[],
        related: &["vt", "vi"],
    },
    Relations {
        id: "hon",
        requires: &[],
        contrasts: &["hum"],
        related: &["pol"],
    },
    Relations {
        id: "hum",
        requires: &[],
        contrasts: &["hon"],
        related: &["pol"],
    },
];

const EXAMPLES: &[(&str, &str, &str)] = &[
    (
        "causative",
        "食べる → 食べさせる",
        "to eat → to make or let someone eat",
    ),
    ("passive", "見る → 見られる", "to see → to be seen"),
    (
        "causative-passive",
        "食べる → 食べさせられる",
        "to eat → to be made to eat",
    ),
    ("potential", "読む → 読める", "to read → can read"),
    ("conjunctive", "食べてから", "after eating"),
    ("vt", "ドアを開ける", "open the door"),
    ("vi", "ドアが開く", "the door opens"),
];

/// The kinds of thing the glossary explains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A form a verb or adjective takes. Ids here match the breakdown's `form`.
    Form,
    /// Grammatical vocabulary the panels use in prose.
    Concept,
    /// What a word *is* — the codes on a dictionary sense.
    PartOfSpeech,
    /// How a verb conjugates, and whether it takes an object.
    VerbClass,
    /// How a word is used rather than what it means.
    Usage,
}

impl Kind {
    /// A heading for a group of these.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Form => "Forms",
            Kind::Concept => "Concepts",
            Kind::PartOfSpeech => "Parts of speech",
            Kind::VerbClass => "Verb classes",
            Kind::Usage => "Usage notes",
        }
    }
}

/// The glossary.
///
/// Grammatical forms first, then parts of speech, then the verb classes, then the notes
/// that change how a word is read rather than what it means.
const TERMS: &[Term] = &[
    // ---- Forms a verb or adjective can be in. Ids match the breakdown's `form`. ----
    Term {
        id: "causative",
        label: "Causative",
        aliases: &["causative", "make someone do", "使役"],
        detail: "Someone makes or lets someone else do the action. 食べる → 食べさせる, \
                 to make or let someone eat. Context decides which, and the particle on the \
                 person usually cannot: a transitive verb has already spent を on its \
                 object, leaving に as the only choice whichever is meant. Only where the \
                 verb is intransitive can both appear, and there を leans towards making \
                 and に towards letting.",
        kind: Kind::Form,
    },
    Term {
        id: "passive",
        label: "Passive",
        aliases: &["passive", "受身"],
        detail: "The subject has the action done to them: 見る → 見られる, to be seen. \
                 Japanese also uses it where English would not, for something that happened \
                 to you and you would rather it had not — 雨に降られた.",
        kind: Kind::Form,
    },
    Term {
        id: "causative-passive",
        label: "Causative-passive",
        aliases: &["causative-passive", "causative passive"],
        detail: "Both at once: someone was made to do it. 食べる → 食べさせられる, to be \
                 made to eat. Almost always carries the sense that they did not want to.",
        kind: Kind::Form,
    },
    Term {
        id: "potential",
        label: "Potential",
        aliases: &["potential", "can do", "可能"],
        detail: "Being able to do the action: 読む → 読める, can read. What is read takes \
                 が rather than を more often than not, because the sentence is now about \
                 what is possible rather than what someone does.",
        kind: Kind::Form,
    },
    Term {
        id: "volitional",
        label: "Volitional",
        aliases: &["volitional", "意向形"],
        detail: "Let's, or shall we, or I think I will: 行く → 行こう. With と思う it \
                 becomes an intention rather than an invitation — 行こうと思う, I think \
                 I'll go.",
        kind: Kind::Form,
    },
    Term {
        id: "imperative",
        label: "Imperative",
        aliases: &["imperative", "command form", "命令形"],
        detail: "A bare order: 行く → 行け. Blunt to the point of rudeness in most \
                 conversation, which is why it turns up mainly in signs, quoted speech and \
                 anger.",
        kind: Kind::Form,
    },
    Term {
        id: "desiderative",
        label: "Desiderative",
        aliases: &["desiderative", "たい", "want to"],
        detail: "Wanting to do something: 食べる → 食べたい. It behaves as an い-adjective \
                 from there on — 食べたくない, 食べたかった — and describes your own wish, \
                 not someone else's.",
        kind: Kind::Form,
    },
    Term {
        id: "provisional",
        label: "Provisional",
        aliases: &["provisional", "ば form", "eba"],
        detail: "The 〜ば conditional: 行けば, if one goes. Leans towards a general or \
                 logical if, rather than a particular occasion.",
        kind: Kind::Form,
    },
    Term {
        id: "conditional",
        label: "Conditional",
        aliases: &["conditional", "たら form", "tara"],
        detail: "The 〜たら conditional: 行ったら, if or when one goes. The everyday one, \
                 and the one that can mean *when* something happens rather than *if*.",
        kind: Kind::Form,
    },
    Term {
        id: "alternative",
        label: "Alternative",
        aliases: &["alternative", "たり form", "tari"],
        detail: "Listing representative actions rather than all of them: 読んだり書いたり \
                 する, doing things like reading and writing. Usually paired and closed \
                 with する.",
        kind: Kind::Form,
    },
    Term {
        id: "conjunctive",
        label: "Te-form",
        aliases: &["te-form", "te form", "conjunctive"],
        detail: "The connecting form: 食べて. It joins clauses, and it is what a great many \
                 grammar patterns attach to — 〜ている, 〜てから, 〜てもいい.",
        kind: Kind::Form,
    },
    Term {
        id: "continuative",
        label: "Continuative stem",
        aliases: &["continuative", "masu stem", "連用形", "stem"],
        detail: "The stem you get by taking ます off: 食べ, 読み. A joint rather than a \
                 form — compounds, 〜ながら and 〜たい all attach here.",
        kind: Kind::Form,
    },
    Term {
        id: "adverbial",
        label: "Adverbial",
        aliases: &["adverbial"],
        detail: "An adjective turned into how something is done: 早い → 早く, quickly. \
                 な-adjectives take に instead — 静かに.",
        kind: Kind::Form,
    },
    Term {
        id: "past",
        label: "Past",
        aliases: &["past", "past tense", "た form"],
        detail: "It happened: 食べた. Japanese marks the verb at the end of the clause, so \
                 one た can put a whole sentence in the past.",
        kind: Kind::Form,
    },
    Term {
        id: "non-past",
        label: "Non-past",
        aliases: &["non-past", "nonpast", "present tense"],
        detail: "One form for the present and the future both, which is why it is not \
                 called present: 食べる is *eats* or *will eat*, and the sentence decides.",
        kind: Kind::Form,
    },
    Term {
        id: "adjective-stem",
        label: "Adjective stem",
        aliases: &["adjective-stem", "adjective stem"],
        detail: "What is left of an い-adjective without its ending — 高い → 高. Endings \
                 attach here: 高くない, 高かった.",
        kind: Kind::Form,
    },
    // ---- Terms the panels use in prose. ----
    Term {
        id: "predicate",
        label: "Predicate",
        aliases: &["predicate", "predicates"],
        detail: "The part that says what is going on — the verb, adjective or noun-plus-copula \
                 the sentence is built around. In Japanese it comes last, so the sentence is \
                 not finished until you have it.",
        kind: Kind::Concept,
    },
    Term {
        id: "copula",
        label: "Copula",
        aliases: &["copula", "だ", "です"],
        detail: "だ and です, the *is* that links a noun to the sentence. Not a verb, though \
                 it takes a similar set of endings: だった, ではない.",
        kind: Kind::Concept,
    },
    Term {
        id: "transitive-pair",
        label: "Transitivity pair",
        aliases: &["transitive-pair", "transitivity pair", "transitivity"],
        detail: "Japanese has matched pairs where English reuses one word: 開く (it opens) \
                 beside 開ける (to open it). Picking the wrong one of a pair is one of the \
                 commonest ways a sentence goes subtly wrong.",
        kind: Kind::Concept,
    },
    Term {
        id: "nominaliser",
        label: "Nominaliser",
        aliases: &["nominaliser", "nominalizer", "nominalisation"],
        detail: "Something that turns a clause into a noun so the rest of the sentence can \
                 use it — こと and の. 泳ぐのが好き: the swimming is what is liked.",
        kind: Kind::Concept,
    },
    Term {
        id: "adnominal",
        label: "Adnominal",
        aliases: &["adnominal", "prenominal", "attributive"],
        detail: "Sitting in front of a noun and describing it. Any Japanese clause can do \
                 this without a relative pronoun: 私が読んだ本, the book I read.",
        kind: Kind::Concept,
    },
    // ---- Parts of speech: the codes on a dictionary sense. ----
    Term {
        id: "n",
        label: "Noun",
        aliases: &["noun", "n"],
        detail: "Names a thing, person or idea. Japanese nouns do not inflect and carry no \
                 article or plural — 本 is a book, the book or books, and the sentence \
                 settles which.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "prt",
        label: "Particle",
        aliases: &["particle", "particles", "prt", "助詞"],
        detail: "A short marker that says what part the word before it plays — は, が, を, \
                 に, で. It carries the grammar that English gets from word order and \
                 prepositions.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "conj",
        label: "Conjunction",
        aliases: &["conjunction", "conjunctions", "conj"],
        detail: "Joins clauses or sentences. Most Japanese conjunctions open the second \
                 clause rather than sitting between the two — そして, だから, それで. A few \
                 attach to the end of the first instead, けど and けども among them.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "adv",
        label: "Adverb",
        aliases: &["adverb", "adverbs", "adv"],
        detail: "Says how, when or how much. Japanese adverbs are loose about position but \
                 come before what they modify — ゆっくり歩く.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "adj-i",
        label: "い-adjective",
        aliases: &[
            "い-adjective",
            "i-adjective",
            "adj-i",
            "adjective",
            "ku-adj",
            "shiku",
        ],
        detail: "Ends in い and conjugates on its own: 高い → 高くない, 高かった. It needs \
                 no だ to finish a sentence.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "adj-na",
        label: "な-adjective",
        aliases: &["な-adjective", "na-adjective", "adj-na", "na-adj"],
        detail: "Takes な before a noun and だ or です to end a sentence: 静かな部屋, \
                 静かです. Grammatically closer to a noun than to an い-adjective.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "exp",
        label: "Expression",
        aliases: &["expression", "exp", "phrase"],
        detail: "A fixed phrase of more than one word that works as a unit. Looking the \
                 pieces up separately will usually not add up to the meaning.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "int",
        label: "Interjection",
        aliases: &["interjection", "int"],
        detail: "A word thrown in whole — ああ, へえ, ちょっと. It sits outside the sentence's \
                 grammar rather than taking a role in it.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "pn",
        label: "Pronoun",
        aliases: &["pronoun", "pn"],
        detail: "Stands in for a noun — これ, 私, 彼. Japanese drops them wherever context \
                 covers it, so they appear far less than their English equivalents.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "ctr",
        label: "Counter",
        aliases: &["counter", "ctr", "助数詞"],
        detail: "A suffix for counting a particular kind of thing — 三本, 二枚, 五人. Which \
                 counter to use depends on the shape or nature of what is counted.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "aux",
        label: "Auxiliary",
        aliases: &["auxiliary", "aux", "auxiliary verb", "aux-verb", "aux-adj"],
        detail: "Attaches to another word and changes what it says rather than standing on \
                 its own — the れる of the passive, the たい of wanting.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "pref",
        label: "Prefix",
        aliases: &["prefix", "pref"],
        detail: "Attaches to the front of a word — お茶, 不安. Cannot be used alone.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "suf",
        label: "Suffix",
        aliases: &["suffix", "suf"],
        detail: "Attaches to the back of a word — 私たち, 田中さん. Cannot be used alone.",
        kind: Kind::PartOfSpeech,
    },
    // ---- Verb classes and transitivity. ----
    Term {
        id: "vt",
        label: "Transitive",
        aliases: &["transitive", "vt"],
        detail: "Takes a direct object, marked を: 開ける, to open something. Most transitive \
                 verbs have an intransitive twin — see transitivity pair.",
        kind: Kind::VerbClass,
    },
    Term {
        id: "vi",
        label: "Intransitive",
        aliases: &["intransitive", "vi"],
        detail: "Takes no direct object — the thing simply happens: 開く, it opens. Use its \
                 transitive partner 開ける when someone does it to something.",
        kind: Kind::VerbClass,
    },
    Term {
        id: "v1",
        label: "Ichidan verb",
        aliases: &[
            "ichidan",
            "ichidan verb",
            "v1",
            "ru-verb",
            "一段",
            "1-dan",
            "1-dan (spec.)",
        ],
        detail: "Drops る and adds the ending, with nothing before it changing: 食べる → \
                 食べます, 食べない, 食べて. The simpler of the two verb classes.",
        kind: Kind::VerbClass,
    },
    Term {
        id: "v5",
        label: "Godan verb",
        aliases: &[
            "godan",
            "godan verb",
            "v5",
            "u-verb",
            "五段",
            "5-dan",
            "5-dan (spec.)",
            "5-dan (irreg.)",
            "2-dan",
        ],
        detail: "The final kana moves through its row before the ending is added: 読む → \
                 読みます, 読まない, 読んで. The code says which row — v5m is む, v5k is く.",
        kind: Kind::VerbClass,
    },
    Term {
        id: "vs",
        label: "する verb",
        aliases: &[
            "suru verb",
            "vs",
            "する verb",
            "suru",
            "su-verb",
            "vs-s",
            "vs-i",
        ],
        detail: "A noun that becomes a verb with する — 勉強 → 勉強する. The noun can still \
                 be used alone, and the two halves can be separated: 勉強をする.",
        kind: Kind::VerbClass,
    },
    Term {
        id: "vk",
        label: "来る verb",
        aliases: &["kuru", "vk", "来る"],
        detail: "来る, which is irregular on its own: 来る, 来ます, 来ない, 来て — the reading \
                 of the kanji changes with the form.",
        kind: Kind::VerbClass,
    },
    // ---- Notes about how a word is used rather than what it means. ----
    Term {
        id: "uk",
        label: "Usually kana",
        aliases: &["usually kana", "uk", "kana alone", "kana"],
        detail: "The kanji exists but is rarely used in practice. Expect this word in kana \
                 even in writing that uses plenty of kanji elsewhere.",
        kind: Kind::Usage,
    },
    Term {
        id: "hon",
        label: "Honorific",
        aliases: &["honorific", "hon", "sonkeigo", "尊敬語"],
        detail: "Raises the person you are talking about: いらっしゃる for 行く or 来る. \
                 Used of others, never of yourself.",
        kind: Kind::Usage,
    },
    Term {
        id: "hum",
        label: "Humble",
        aliases: &["humble", "hum", "kenjougo", "謙譲語"],
        detail: "Lowers you relative to the person you are speaking to: 参る for 行く. Used \
                 of yourself and your own side, never of them.",
        kind: Kind::Usage,
    },
    Term {
        id: "pol",
        label: "Polite",
        aliases: &["polite", "pol", "teineigo", "丁寧語"],
        detail: "The です／ます register — polite about the conversation rather than about \
                 anyone in particular. Independent of honorific and humble, which are about \
                 who is raised or lowered.",
        kind: Kind::Usage,
    },
    Term {
        id: "col",
        label: "Colloquial",
        aliases: &["colloquial", "col"],
        detail: "Everyday spoken usage. Fine in conversation, out of place in anything \
                 written formally.",
        kind: Kind::Usage,
    },
    Term {
        id: "arch",
        label: "Archaic",
        aliases: &["archaic", "arch", "obsolete", "obs", "dated", "historical"],
        detail: "No longer in ordinary use. Worth recognising in older writing, not worth \
                 producing.",
        kind: Kind::Usage,
    },
    Term {
        id: "on-mim",
        label: "Onomatopoeia",
        aliases: &["onomatopoeia", "on-mim", "mimetic", "擬音語", "擬態語"],
        detail: "A sound or manner word — きらきら, どきどき. Japanese uses these far more \
                 than English does, and for states as well as sounds.",
        kind: Kind::Usage,
    },
    Term {
        id: "id",
        label: "Idiom",
        aliases: &["idiom", "idiomatic", "id"],
        detail: "The whole means something the parts do not. Translate the phrase, not the \
                 words in it.",
        kind: Kind::Usage,
    },
    Term {
        id: "abbr",
        label: "Abbreviation",
        aliases: &["abbreviation", "abbr", "abbr."],
        detail: "A shortened form of a longer word or phrase — スマホ from スマートフォン.",
        kind: Kind::Usage,
    },
    // ---- Added from the tags a real dictionary actually emits, rather than from JMdict's
    // documentation a second time. Jitendex spells its tags out — `5-dan` where JMdict
    // writes `v5` — and this table had been written against the codes, so most of what
    // reaches a badge went unexplained. `every_grammatical_tag_is_explained` measures it.
    Term {
        id: "forms",
        label: "Alternative forms",
        aliases: &["forms", "alternative forms"],
        detail: "The word is written or read more than one way, and the entry lists the \
                 others. Not a part of speech: it says the spelling in front of you is one \
                 of several, not what the word does.",
        kind: Kind::Concept,
    },
    Term {
        id: "adj-no",
        label: "の-adjective",
        aliases: &["adj-no", "no-adj", "の-adjective"],
        detail: "A noun that describes another noun by taking の: 本当 → 本当の話, a true \
                 story. The noun does the work; の is what lets it modify.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "adv-to",
        label: "と-adverb",
        aliases: &["adv-to", "to-adverb", "と-adverb"],
        detail: "Takes と to work as an adverb: ゆっくりと歩く. Many are mimetic words, and \
                 the と is often dropped in speech.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "adj-pn",
        label: "Pre-noun adjectival",
        aliases: &["adj-pn", "adjectival", "pre-noun adjectival", "連体詞"],
        detail: "A word that can only stand in front of a noun — この, その, あの, 大きな. \
                 It does not conjugate and cannot end a sentence, which is what separates \
                 it from an adjective.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "adj-t",
        label: "たる-adjective",
        aliases: &["adj-t", "taru", "たる-adjective"],
        detail: "An older class taking たる before a noun and と as an adverb: 堂々たる, \
                 堂々と. Formal, and mostly literary now.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "num",
        label: "Numeral",
        aliases: &["num", "numeric", "numeral"],
        detail: "A number word. It usually needs a counter to attach to what is being \
                 counted — 一 becomes 一本, 一枚 or 一人 depending on the thing.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "unc",
        label: "Unclassified",
        aliases: &["unc", "unclass", "unclassified"],
        detail: "No part of speech assigned — usually a mark rather than a word, such as \
                 the repetition signs 々 and ヽ.",
        kind: Kind::PartOfSpeech,
    },
    Term {
        id: "vz",
        label: "ずる verb",
        aliases: &["vz", "zuru", "ずる verb"],
        detail: "A する verb whose する has voiced to ずる: 論ずる, 感ずる. It conjugates as \
                 an ichidan verb, and most have a plain じる form beside them — 論じる.",
        kind: Kind::VerbClass,
    },
    Term {
        id: "rare",
        label: "Rare",
        aliases: &["rare", "rarely used"],
        detail: "Recorded, but seldom met. Worth recognising if it turns up; not worth \
                 reaching for.",
        kind: Kind::Usage,
    },
    Term {
        id: "formal",
        label: "Formal",
        aliases: &["formal"],
        detail: "For writing and set-piece speech — documents, announcements, ceremonies. \
                 Separate from polite, which is about the person you are speaking to.",
        kind: Kind::Usage,
    },
    Term {
        id: "sl",
        label: "Slang",
        aliases: &[
            "sl",
            "slang",
            "net slang",
            "net-sl",
            "internet slang",
            "m-sl",
        ],
        detail: "Current, informal and quick to date. Fine among friends, out of place \
                 anywhere it would be written down.",
        kind: Kind::Usage,
    },
    Term {
        id: "derog",
        label: "Derogatory",
        aliases: &["derog", "derogatory"],
        detail: "Insulting about whoever it describes. Worth knowing when you meet it, and \
                 worth not using.",
        kind: Kind::Usage,
    },
    Term {
        id: "vulg",
        label: "Vulgar",
        aliases: &["vulg", "vulgar"],
        detail: "Coarse. Recorded so it can be read, not so it can be said.",
        kind: Kind::Usage,
    },
    Term {
        id: "sens",
        label: "Sensitive",
        aliases: &["sens", "sensitive"],
        detail: "Touches something people are careful about — illness, origin, disability. \
                 There is usually a neutral word beside it that is the safer choice.",
        kind: Kind::Usage,
    },
    Term {
        id: "euph",
        label: "Euphemism",
        aliases: &["euph", "euphemism"],
        detail: "A gentler way of saying something people would rather not say directly.",
        kind: Kind::Usage,
    },
    Term {
        id: "joc",
        label: "Jocular",
        aliases: &["joc", "jocular", "humorous"],
        detail: "Said in fun. It will read as a joke whether or not one was meant.",
        kind: Kind::Usage,
    },
    Term {
        id: "fam",
        label: "Familiar",
        aliases: &["fam", "familiar"],
        detail: "For people you are close to. Warm among friends, presumptuous with anyone \
                 else.",
        kind: Kind::Usage,
    },
    Term {
        id: "chn",
        label: "Children's language",
        aliases: &["chn", "childish", "childrens language"],
        detail: "What children say, or what adults say to them — わんわん for a dog. Between \
                 adults it is baby talk.",
        kind: Kind::Usage,
    },
    Term {
        id: "male",
        label: "Masculine",
        aliases: &["male", "masculine", "male term"],
        detail: "Marked as men's speech. Japanese carries more of this than English does, \
                 in pronouns and sentence endings especially.",
        kind: Kind::Usage,
    },
    Term {
        id: "fem",
        label: "Feminine",
        aliases: &["fem", "feminine", "female term"],
        detail: "Marked as women's speech — the counterpart to the masculine label, and \
                 like it, softening as the language changes.",
        kind: Kind::Usage,
    },
    Term {
        id: "poet",
        label: "Poetical",
        aliases: &["poet", "poetical", "poetic"],
        detail: "Belongs to verse and to writing that reaches for it. Striking in a poem, \
                 overwrought in a conversation.",
        kind: Kind::Usage,
    },
    Term {
        id: "proverb",
        label: "Proverb",
        aliases: &["proverb"],
        detail: "A saying quoted whole. The words are fixed, so translate what it means \
                 rather than what it says.",
        kind: Kind::Usage,
    },
    Term {
        id: "quote",
        label: "Quotation",
        aliases: &["quote", "quotation"],
        detail: "A line quoted from a work, a speech or a person, carrying whatever the \
                 source gave it.",
        kind: Kind::Usage,
    },
    Term {
        id: "yoji",
        label: "Four-character idiom",
        aliases: &["yoji", "yojijukugo", "四字熟語"],
        detail: "A fixed phrase of exactly four kanji, usually with a classical source — \
                 一石二鳥, one stone two birds. The four are the unit; reading them \
                 separately will not give the meaning.",
        kind: Kind::Usage,
    },
];

fn index() -> &'static HashMap<&'static str, &'static Term> {
    static INDEX: OnceLock<HashMap<&'static str, &'static Term>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map = HashMap::new();
        for term in TERMS {
            map.insert(term.id, term);
            for alias in term.aliases {
                map.insert(*alias, term);
            }
        }
        map
    })
}

/// Every entry, in table order.
pub fn all() -> &'static [Term] {
    TERMS
}

/// Every entry in the richer presentation contract, in the table's canonical order.
pub fn all_views() -> Vec<TermView> {
    TERMS.iter().map(view).collect()
}

/// One enriched entry by id or alias.
pub fn get_view(key: &str) -> Option<TermView> {
    get(key).map(view)
}

/// Enriched search results, preserving the search table's deterministic order.
pub fn search_views(query: &str) -> Vec<TermView> {
    search(query).into_iter().map(view).collect()
}

fn view(term: &Term) -> TermView {
    let (summary, detail) = split_detail(term.detail);
    let relations = RELATIONS.iter().find(|relations| relations.id == term.id);
    let ids = |pick: fn(&Relations) -> &'static [&'static str]| {
        relations
            .map(pick)
            .unwrap_or_default()
            .iter()
            .map(|id| (*id).to_string())
            .collect()
    };
    TermView {
        id: term.id.to_string(),
        label: term.label.to_string(),
        aliases: term
            .aliases
            .iter()
            .map(|alias| (*alias).to_string())
            .collect(),
        summary: summary.to_string(),
        detail: detail.map(str::to_string),
        examples: EXAMPLES
            .iter()
            .filter(|(id, _, _)| *id == term.id)
            .map(|(_, japanese, english)| TermExample {
                japanese: (*japanese).to_string(),
                english: (*english).to_string(),
            })
            .collect(),
        requires: ids(|relations| relations.requires),
        contrasts: ids(|relations| relations.contrasts),
        related: ids(|relations| relations.related),
        kind: term.kind,
    }
}

/// The first sentence is enough to choose a term; the remainder belongs in its detail.
/// Split only at sentence punctuation followed by whitespace, so abbreviations do not create
/// an accidental fragment.
fn split_detail(detail: &str) -> (&str, Option<&str>) {
    let boundary = detail.char_indices().find_map(|(at, character)| {
        if !matches!(character, '.' | '!' | '?' | '。') {
            return None;
        }
        let end = at + character.len_utf8();
        let is_boundary = detail[end..].chars().next().is_none_or(char::is_whitespace);
        is_boundary.then_some(end)
    });
    match boundary {
        Some(end) if end < detail.len() => {
            let rest = detail[end..].trim();
            (&detail[..end], (!rest.is_empty()).then_some(rest))
        }
        _ => (detail, None),
    }
}

/// One entry by its id or any of its aliases, case-insensitively.
pub fn get(key: &str) -> Option<&'static Term> {
    let key = key.trim();
    index()
        .get(key)
        .copied()
        .or_else(|| index().get(key.to_lowercase().as_str()).copied())
}

/// Entries a search should offer for `query`.
///
/// An exact id or alias first, then anything whose alias or label starts with the query, so
/// a half-typed "caus" still finds the causative. Never a substring match anywhere in the
/// detail: a search for "verb" would otherwise return most of the table.
pub fn search(query: &str) -> Vec<&'static Term> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }

    let mut found: Vec<&'static Term> = Vec::new();
    let mut push = |term: &'static Term| {
        if !found.iter().any(|seen| seen.id == term.id) {
            found.push(term);
        }
    };

    if let Some(exact) = get(&query) {
        push(exact);
    }
    for term in TERMS {
        let starts = term
            .aliases
            .iter()
            .any(|alias| alias.to_lowercase().starts_with(&query))
            || term.label.to_lowercase().starts_with(&query);
        if starts {
            push(term);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_entry_is_filled_in() {
        for term in all() {
            assert!(!term.id.is_empty(), "an entry has no id");
            assert!(!term.label.is_empty(), "{} has no label", term.id);
            assert!(!term.detail.is_empty(), "{} has no detail", term.id);
            assert!(!term.aliases.is_empty(), "{} has no aliases", term.id);
            assert!(
                term.aliases.contains(&term.id),
                "{} does not list its own id as an alias, so searching for the code that \
                 appears on screen would not find it",
                term.id
            );
        }
    }

    /// The kinds are what the browsable list groups by, so an entry filed under the wrong
    /// one is a term that turns up in the wrong section and nowhere else. These spot-check
    /// one from each group against the section of the table it is written in.
    #[test]
    fn entries_are_filed_under_the_group_they_are_written_in() {
        for (id, expected) in [
            ("causative", Kind::Form),
            ("predicate", Kind::Concept),
            ("conj", Kind::PartOfSpeech),
            ("vi", Kind::VerbClass),
            ("uk", Kind::Usage),
        ] {
            assert_eq!(
                get(id).map(|term| term.kind),
                Some(expected),
                "{id} is misfiled"
            );
        }
    }

    /// Every group has to be reachable, or a heading in the list would head nothing.
    #[test]
    fn no_group_is_empty() {
        for kind in [
            Kind::Form,
            Kind::Concept,
            Kind::PartOfSpeech,
            Kind::VerbClass,
            Kind::Usage,
        ] {
            assert!(
                all().iter().any(|term| term.kind == kind),
                "nothing is filed under {}",
                kind.label()
            );
        }
    }

    /// A duplicate id would make `get` return whichever the map happened to keep, and a
    /// duplicate alias would make a search silently ambiguous. Both are easy to introduce
    /// in a table this long and invisible without a test.
    #[test]
    fn ids_and_aliases_are_unique() {
        let mut seen: HashSet<&str> = HashSet::new();
        for term in all() {
            assert!(seen.insert(term.id), "duplicate id {}", term.id);
        }
        let mut claimed: HashSet<String> = HashSet::new();
        for term in all() {
            for alias in term.aliases {
                let alias = alias.to_lowercase();
                if alias == term.id {
                    continue;
                }
                assert!(
                    claimed.insert(alias.clone()),
                    "two terms both answer to {alias:?}"
                );
            }
        }
    }

    /// Pull the keys of a table out of a TypeScript or Svelte source file.
    ///
    /// Reading the real file rather than copying it. The point of these two tests is that
    /// the interface and this glossary agree, and a hand-copied list cannot show that —
    /// it only shows that the copy agrees with itself, and it goes stale the first time
    /// someone edits the original without knowing a duplicate exists.
    fn keys_of(file: &str, opening: &str) -> Vec<String> {
        let source = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {file}: {e} — has the interface moved?"));
        let body = source
            .split_once(opening)
            .unwrap_or_else(|| panic!("{opening:?} is no longer in {file}"))
            .1;
        let mut keys = Vec::new();
        for line in body.lines() {
            let line = line.trim();
            if line.starts_with("};") || line.starts_with("];") {
                break;
            }
            if line.starts_with("//") {
                continue;
            }
            // Two shapes: an object key, `"non-past": "…"` or `causative: "…"`, where the
            // key is what precedes the colon; and an array element, `{ kind: "form", label:
            // "Forms" },` where it is the first field's value. Both then run to a comma.
            let Some(key) = line
                .strip_prefix("{ kind:")
                .or_else(|| line.split_once(':').map(|(k, _)| k))
            else {
                continue;
            };
            let key = key.split(',').next().unwrap().trim().trim_matches('"');
            if !key.is_empty() {
                keys.push(key.to_string());
            }
        }
        assert!(
            !keys.is_empty(),
            "read no keys out of {file} — the parse is wrong"
        );
        keys
    }

    const GRAMMAR_ORDER: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ui/src/lib/grammar-order.js"
    );
    const MODEL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/model.ts");

    /// The breakdown labels its steps by `form`, and the panel looks the term up by that
    /// same string. Every form the panel offers as a button must therefore have a term —
    /// and it must be a *form*, not something else that happens to answer to the name.
    ///
    /// The kind check is the half that earns its keep. `get` matches aliases as well as
    /// ids, so a form named after an existing alias resolves to a real term and the
    /// existence check passes while the app shows an answer to a different question:
    /// `unclassified` used to be listed here and explained itself as the `unc` part of
    /// speech, telling a reader about the repetition mark 々.
    #[test]
    fn every_form_the_breakdown_can_show_has_a_term() {
        for form in keys_of(GRAMMAR_ORDER, "export const FORM_TITLES = {") {
            let term = get(&form).unwrap_or_else(|| panic!("no term for the form {form:?}"));
            assert_eq!(
                term.kind,
                Kind::Form,
                "the form {form:?} resolves to {:?}, which is a {} rather than a form",
                term.id,
                term.kind.label()
            );
        }
    }

    /// The glossary groups its terms under headings held in `TERM_KINDS`, and that list is
    /// written out again in TypeScript because the type cannot cross the boundary. The
    /// grouped view drops any section that comes out empty, so a sixth `Kind` added here
    /// and not there would make every term under it disappear from the page with no
    /// heading, no error, and nothing on screen to suggest anything was missing.
    #[test]
    fn the_interface_has_a_heading_for_every_kind() {
        let headings = keys_of(
            MODEL,
            "export const TERM_KINDS: { kind: TermKind; label: string }[] = [",
        );
        for term in all() {
            // Through serde, because that is how the kind actually reaches the interface —
            // comparing anything else would be testing a second translation of our own.
            let kind = serde_json::to_value(term.kind).unwrap();
            let kind = kind.as_str().unwrap();
            assert!(
                headings.iter().any(|h| h == kind),
                "{:?} is a {kind}, and the glossary page has no heading for that — every                  term in the group would vanish from it",
                term.id
            );
        }
        for heading in &headings {
            assert!(
                all()
                    .iter()
                    .any(|t| serde_json::to_value(t.kind).unwrap() == **heading),
                "the glossary page lists a {heading:?} heading, but no term is one"
            );
        }
    }

    #[test]
    fn a_code_a_sense_carries_finds_its_term() {
        assert_eq!(get("conj").map(|t| t.label), Some("Conjunction"));
        assert_eq!(get("vi").map(|t| t.label), Some("Intransitive"));
        assert_eq!(
            get("N").map(|t| t.label),
            Some("Noun"),
            "codes arrive lowercase but a search may not"
        );
    }

    #[test]
    fn searching_finds_a_term_before_it_is_fully_typed() {
        let found = search("caus");
        assert!(
            found.iter().any(|term| term.id == "causative"),
            "half-typed query found {:?}",
            found.iter().map(|t| t.id).collect::<Vec<_>>()
        );
        assert!(search("").is_empty());
    }

    /// Prefix matching only. A substring search would make "verb" return most of the table
    /// through the detail text, which is noise rather than an answer.
    #[test]
    fn a_word_buried_in_a_detail_is_not_a_match() {
        let found = search("sentence");
        assert!(
            found.is_empty(),
            "matched on detail text: {:?}",
            found.iter().map(|t| t.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn enriched_views_preserve_every_term_and_all_of_its_prose() {
        let views = all_views();
        assert_eq!(views.len(), all().len());
        for (term, view) in all().iter().zip(views) {
            assert_eq!(term.id, view.id);
            let rebuilt = match view.detail {
                Some(detail) => format!("{} {}", view.summary, detail),
                None => view.summary,
            };
            assert_eq!(term.detail, rebuilt, "{} lost explanation text", term.id);
        }
    }

    #[test]
    fn reviewed_relations_are_valid_and_requirements_are_acyclic() {
        for relation in RELATIONS {
            assert!(
                get(relation.id).is_some(),
                "unknown relation owner {}",
                relation.id
            );
            let mut targets = HashSet::new();
            for target in relation
                .requires
                .iter()
                .chain(relation.contrasts)
                .chain(relation.related)
            {
                assert_ne!(relation.id, *target, "{} links to itself", relation.id);
                assert!(
                    get(target).is_some(),
                    "{} links to missing {target}",
                    relation.id
                );
                assert!(targets.insert(*target), "{} repeats {target}", relation.id);
            }
            for target in relation.contrasts {
                let reverse = RELATIONS
                    .iter()
                    .find(|candidate| candidate.id == *target)
                    .map(|candidate| candidate.contrasts.contains(&relation.id))
                    .unwrap_or(false);
                assert!(
                    reverse,
                    "{} contrasts with {target}, but not conversely",
                    relation.id
                );
            }
        }

        fn visit(id: &str, visiting: &mut Vec<String>, done: &mut HashSet<String>) {
            if done.contains(id) {
                return;
            }
            assert!(
                !visiting.iter().any(|seen| seen == id),
                "requires cycle at {id}"
            );
            visiting.push(id.to_string());
            if let Some(relation) = RELATIONS.iter().find(|relation| relation.id == id) {
                for requirement in relation.requires {
                    visit(requirement, visiting, done);
                }
            }
            visiting.pop();
            done.insert(id.to_string());
        }

        let mut done = HashSet::new();
        for term in all() {
            visit(term.id, &mut Vec::new(), &mut done);
        }
    }
}
