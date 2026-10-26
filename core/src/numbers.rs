//! Reading a number written in digits, with the counter that follows it.
//!
//! # Why this exists
//!
//! A counted number is composed from its parts, and its parts do not spell it. 34歳 arrived as
//! 3 + 4 + 歳 and was read さん + よん + さい — **さんよんさい**, which is not a number anybody says.
//! 20分 was にぜろふん, 15分 いちごふん, 1963年 いちきゅうろくさんねん. The place values were simply
//! never assembled.
//!
//! Kanji numerals do not have this problem, and the difference says exactly what is missing: 一回
//! and 九時 are Jitendex entries, so `dict::readings` supplies いっかい and くじ for the whole. There
//! is no entry for 34歳, and there could not sensibly be one for every number, so the reading has to
//! be **derived** — which is what this does.
//!
//! # What it will not do
//!
//! **A digit string is not always a cardinal**, and this refuses the cases where it is not rather
//! than inventing a confident reading:
//!
//! - A leading zero means an identifier, not a quantity: `007` is not ななばん's seven.
//! - A run longer than [`MAX_DIGITS`] is a card or account number, not a count.
//! - An unrecognised counter gets the plain cardinal and the counter's own reading — correct place
//!   values, no invented euphony. It is not a licence to guess at alternations nobody recorded.
//!
//! The caller keeps the existing rule that a number with **no** counter gets no reading at all,
//! because 2011 is にせんじゅういち as a year and にぜろいちいち as four digits and nothing here can
//! tell which.
//!
//! # Where the irregularity lives
//!
//! Place values are regular and are computed. Counter euphony is not: 3本 is さんぼん while 3分 is
//! さんぷん and 3枚 is さんまい, all three of them は行-or-not counters after the same number. So the
//! alternations are **recorded per counter** in [`COUNTERS`], and a counter that is not there falls
//! through to no alternation rather than to a guess.

/// The longest digit run read as a quantity.
///
/// Sixteen covers 兆 with room to spare, and stops short of the lengths that are identifiers rather
/// than numbers — card numbers, account numbers, timestamps.
pub const MAX_DIGITS: usize = 16;

/// 1-9, standing alone.
const UNITS: [&str; 9] = [
    "いち",
    "に",
    "さん",
    "よん",
    "ご",
    "ろく",
    "なな",
    "はち",
    "きゅう",
];

/// The counter's reading in one numeric environment, and what the number does in front of it.
struct Env {
    /// The number this applies to, matched against the value's tail: 1-9 as themselves, and 10 for
    /// a value ending in a bare 十 (10, 20, 30 …).
    tail: u8,
    /// The number's own last element, where the counter changes it — いち becomes いっ before 回,
    /// じゅう becomes じゅっ before 分. `None` leaves the cardinal alone.
    number: Option<&'static str>,
    /// The counter's reading here.
    counter: &'static str,
}

/// A counter, its plain reading, and everything about it that is not regular.
struct Counter {
    surface: &'static str,
    /// The reading used where nothing alternates.
    plain: &'static str,
    /// Environments, consulted by the value's tail.
    envs: &'static [Env],
    /// Values whose **whole** reading — number and counter together — is its own word. 一日 is
    /// ついたち, and no rule assembles that from anything.
    whole: &'static [(u64, &'static str)],
}

/// The counters this supports, with the alternations each one actually has.
///
/// **Deliberately a table and deliberately short.** Counter euphony is lexical: 3本 さんぼん, 3分
/// さんぷん, 3枚 さんまい and 3階 さんがい are four different answers to the same environment, so no
/// rule derives them and a rule that appeared to would be wrong somewhere it was never checked.
/// A counter absent from here keeps its own reading, which is the honest result.
const COUNTERS: &[Counter] = &[
    Counter {
        surface: "歳",
        plain: "さい",
        envs: &[
            Env {
                tail: 1,
                number: Some("いっ"),
                counter: "さい",
            },
            Env {
                tail: 8,
                number: Some("はっ"),
                counter: "さい",
            },
            Env {
                tail: 10,
                number: Some("じゅっ"),
                counter: "さい",
            },
        ],
        // 二十歳 is はたち, and it is written 20歳 as often as not.
        whole: &[(20, "はたち")],
    },
    Counter {
        surface: "才",
        plain: "さい",
        envs: &[
            Env {
                tail: 1,
                number: Some("いっ"),
                counter: "さい",
            },
            Env {
                tail: 8,
                number: Some("はっ"),
                counter: "さい",
            },
            Env {
                tail: 10,
                number: Some("じゅっ"),
                counter: "さい",
            },
        ],
        whole: &[(20, "はたち")],
    },
    Counter {
        surface: "分",
        plain: "ふん",
        envs: &[
            Env {
                tail: 1,
                number: Some("いっ"),
                counter: "ぷん",
            },
            Env {
                tail: 3,
                number: None,
                counter: "ぷん",
            },
            Env {
                tail: 4,
                number: None,
                counter: "ぷん",
            },
            Env {
                tail: 6,
                number: Some("ろっ"),
                counter: "ぷん",
            },
            Env {
                tail: 8,
                number: Some("はっ"),
                counter: "ぷん",
            },
            Env {
                tail: 10,
                number: Some("じゅっ"),
                counter: "ぷん",
            },
        ],
        whole: &[],
    },
    Counter {
        surface: "本",
        plain: "ほん",
        envs: &[
            Env {
                tail: 1,
                number: Some("いっ"),
                counter: "ぽん",
            },
            Env {
                tail: 3,
                number: None,
                counter: "ぼん",
            },
            Env {
                tail: 6,
                number: Some("ろっ"),
                counter: "ぽん",
            },
            Env {
                tail: 8,
                number: Some("はっ"),
                counter: "ぽん",
            },
            Env {
                tail: 10,
                number: Some("じゅっ"),
                counter: "ぽん",
            },
        ],
        whole: &[],
    },
    Counter {
        surface: "匹",
        plain: "ひき",
        envs: &[
            Env {
                tail: 1,
                number: Some("いっ"),
                counter: "ぴき",
            },
            Env {
                tail: 3,
                number: None,
                counter: "びき",
            },
            Env {
                tail: 6,
                number: Some("ろっ"),
                counter: "ぴき",
            },
            Env {
                tail: 8,
                number: Some("はっ"),
                counter: "ぴき",
            },
            Env {
                tail: 10,
                number: Some("じゅっ"),
                counter: "ぴき",
            },
        ],
        whole: &[],
    },
    Counter {
        surface: "回",
        plain: "かい",
        envs: &[
            Env {
                tail: 1,
                number: Some("いっ"),
                counter: "かい",
            },
            Env {
                tail: 6,
                number: Some("ろっ"),
                counter: "かい",
            },
            Env {
                tail: 8,
                number: Some("はっ"),
                counter: "かい",
            },
            Env {
                tail: 10,
                number: Some("じゅっ"),
                counter: "かい",
            },
        ],
        whole: &[],
    },
    Counter {
        surface: "階",
        plain: "かい",
        envs: &[
            Env {
                tail: 1,
                number: Some("いっ"),
                counter: "かい",
            },
            Env {
                tail: 3,
                number: None,
                counter: "がい",
            },
            Env {
                tail: 6,
                number: Some("ろっ"),
                counter: "かい",
            },
            Env {
                tail: 8,
                number: Some("はっ"),
                counter: "かい",
            },
            Env {
                tail: 10,
                number: Some("じゅっ"),
                counter: "かい",
            },
        ],
        whole: &[],
    },
    Counter {
        surface: "冊",
        plain: "さつ",
        envs: &[
            Env {
                tail: 1,
                number: Some("いっ"),
                counter: "さつ",
            },
            Env {
                tail: 8,
                number: Some("はっ"),
                counter: "さつ",
            },
            Env {
                tail: 10,
                number: Some("じゅっ"),
                counter: "さつ",
            },
        ],
        whole: &[],
    },
    Counter {
        surface: "個",
        plain: "こ",
        envs: &[
            Env {
                tail: 1,
                number: Some("いっ"),
                counter: "こ",
            },
            Env {
                tail: 6,
                number: Some("ろっ"),
                counter: "こ",
            },
            Env {
                tail: 8,
                number: Some("はっ"),
                counter: "こ",
            },
            Env {
                tail: 10,
                number: Some("じゅっ"),
                counter: "こ",
            },
        ],
        whole: &[],
    },
    Counter {
        // **The native series, and the reason this table has a `whole` column at all.** ～つ does
        // not count in Sino-Japanese: 一つ is ひとつ, not いちつ. UniDic already knew — it gives the
        // digit itself a contextual reading — and an earlier version of this module overrode that
        // with いち, turning ひとつ into いちつ. Caught by measuring what the change moved rather
        // than by a test, which is why the measurement is in the ledger.
        //
        // Above ten there is no native form in ordinary use, so the plain Sino-Japanese counter
        // takes over: 十一個 rather than anything built from とお.
        surface: "つ",
        plain: "つ",
        envs: &[],
        whole: &[
            (1, "ひとつ"),
            (2, "ふたつ"),
            (3, "みっつ"),
            (4, "よっつ"),
            (5, "いつつ"),
            (6, "むっつ"),
            (7, "ななつ"),
            (8, "やっつ"),
            (9, "ここのつ"),
            (10, "とお"),
        ],
    },
    Counter {
        surface: "枚",
        plain: "まい",
        envs: &[],
        whole: &[],
    },
    Counter {
        surface: "年",
        plain: "ねん",
        envs: &[Env {
            tail: 4,
            number: Some("よ"),
            counter: "ねん",
        }],
        whole: &[],
    },
    // **週間, added on evidence rather than on the engine that disagreed.**
    //
    // The colloquial pilot found 1週間 read いちしゅうかん, because the counter was absent here and
    // the cardinal was simply concatenated with the counter's own reading. Ichiran said
    // いっしゅうかん, but Ichiran is the thing under comparison, so its opinion is not the basis.
    //
    // What the installed dictionaries record directly: **一週間 = いっしゅうかん** and
    // **二週間 = にしゅうかん**. So the sokuon is real at 1, and 2 takes the plain form, which is
    // what an empty environment already gives.
    //
    // Tails 8 and 10 are the **same alternation family, not separate guesses**: 週間 begins with
    // し, and the /s/-initial counters already in this table — 歳 and 才 — carry exactly 1, 8 and
    // 10, with the dictionaries confirming 一歳 いっさい and 十歳 じっさい/じゅっさい. They rest on
    // that family plus the direct attestation at 1, and the dictionaries hold no 八週間 or 十週間
    // entry to check them against. Recorded so a later reader knows which rows are attested and
    // which are analogy.
    //
    // Longer numbers need nothing extra: the environment is chosen by `value % 10`, so 11週間
    // reaches tail 1 and becomes じゅういっしゅうかん by the same path as 一週間.
    Counter {
        surface: "週間",
        plain: "しゅうかん",
        envs: &[
            Env {
                tail: 1,
                number: Some("いっ"),
                counter: "しゅうかん",
            },
            Env {
                tail: 8,
                number: Some("はっ"),
                counter: "しゅうかん",
            },
            Env {
                tail: 10,
                number: Some("じゅっ"),
                counter: "しゅうかん",
            },
        ],
        whole: &[],
    },
    Counter {
        surface: "時",
        plain: "じ",
        envs: &[
            Env {
                tail: 4,
                number: Some("よ"),
                counter: "じ",
            },
            Env {
                tail: 7,
                number: Some("しち"),
                counter: "じ",
            },
            Env {
                tail: 9,
                number: Some("く"),
                counter: "じ",
            },
        ],
        whole: &[],
    },
    Counter {
        surface: "人",
        plain: "にん",
        envs: &[Env {
            tail: 4,
            number: Some("よ"),
            counter: "にん",
        }],
        // 一人 and 二人 are their own words, not a number plus a counter.
        whole: &[(1, "ひとり"), (2, "ふたり")],
    },
    Counter {
        surface: "日",
        plain: "にち",
        // Above the irregular run, 7 and 9 keep their older readings: 17日 is じゅうしちにち and
        // 19日 じゅうくにち, never じゅうなな / じゅうきゅう. `whole` is consulted first, so 7日 and
        // 9日 still reach なのか and ここのか rather than these.
        envs: &[
            Env {
                tail: 7,
                number: Some("しち"),
                counter: "にち",
            },
            Env {
                tail: 9,
                number: Some("く"),
                counter: "にち",
            },
        ],
        // **Almost entirely irregular below 11**, which is why those are listed whole: the
        // readings are not the cardinal plus anything.
        whole: &[
            (1, "ついたち"),
            (2, "ふつか"),
            (3, "みっか"),
            (4, "よっか"),
            (5, "いつか"),
            (6, "むいか"),
            (7, "なのか"),
            (8, "ようか"),
            (9, "ここのか"),
            (10, "とおか"),
            (14, "じゅうよっか"),
            (20, "はつか"),
            (24, "にじゅうよっか"),
        ],
    },
    Counter {
        surface: "月",
        plain: "がつ",
        // The month names: 4月 しがつ, 7月 しちがつ, 9月 くがつ are not よん / なな / きゅう.
        envs: &[
            Env {
                tail: 4,
                number: Some("し"),
                counter: "がつ",
            },
            Env {
                tail: 7,
                number: Some("しち"),
                counter: "がつ",
            },
            Env {
                tail: 9,
                number: Some("く"),
                counter: "がつ",
            },
        ],
        whole: &[],
    },
];

/// The digits of `text` as a quantity, or `None` when it is not one.
///
/// Accepts ASCII and the fullwidth forms together, because a passage mixes them freely and the
/// distinction is typographic. Refuses a leading zero on a multi-digit run and anything longer than
/// [`MAX_DIGITS`] — see the module note on what is not a cardinal.
pub fn parse_digits(text: &str) -> Option<u64> {
    if text.is_empty() || text.chars().count() > MAX_DIGITS {
        return None;
    }
    let mut digits = String::with_capacity(text.len());
    for character in text.chars() {
        let digit = match character {
            '0'..='9' => character,
            '０'..='９' => char::from_u32(character as u32 - 0xFEE0)?,
            _ => return None,
        };
        digits.push(digit);
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    digits.parse().ok()
}

/// The Sino-Japanese reading of a non-negative integer.
///
/// Place values are regular and the irregularities are the well-known ones: 300 さんびゃく, 600
/// ろっぴゃく, 800 はっぴゃく, 3000 さんぜん, 8000 はっせん, and a leading 一 that is spoken before
/// 万 and above but not before 十, 百 or 千.
pub fn cardinal(value: u64) -> Option<String> {
    if value == 0 {
        return Some("ぜろ".to_string());
    }
    // 万, 億, 兆 — and the bare group below them.
    const SCALES: [(u64, &str); 3] = [
        (1_000_000_000_000, "ちょう"),
        (100_000_000, "おく"),
        (10_000, "まん"),
    ];

    let mut out = String::new();
    let mut left = value;
    for (size, name) in SCALES {
        if left < size {
            continue;
        }
        let count = left / size;
        left %= size;
        // **一 is spoken here and not below.** 一万 is いちまん where 百 is ひゃく, so the group
        // reading is used as-is rather than dropping its leading unit.
        out.push_str(&group(count)?);
        // 一兆 is いっちょう: the same 促音 the か行 counters take.
        if name == "ちょう" && count % 10 == 1 && count == 1 {
            out.truncate(out.len() - "いち".len());
            out.push_str("いっ");
        }
        out.push_str(name);
    }
    if left > 0 {
        out.push_str(&group(left)?);
    }
    Some(out)
}

/// One group of up to four digits, 1-9999.
fn group(value: u64) -> Option<String> {
    if value == 0 || value > 9999 {
        return None;
    }
    let mut out = String::new();
    let thousands = value / 1000;
    let hundreds = (value / 100) % 10;
    let tens = (value / 10) % 10;
    let units = value % 10;

    if thousands > 0 {
        match thousands {
            1 => out.push_str("せん"),
            3 => out.push_str("さんぜん"),
            8 => out.push_str("はっせん"),
            other => {
                out.push_str(UNITS[other as usize - 1]);
                out.push_str("せん");
            }
        }
    }
    if hundreds > 0 {
        match hundreds {
            1 => out.push_str("ひゃく"),
            3 => out.push_str("さんびゃく"),
            6 => out.push_str("ろっぴゃく"),
            8 => out.push_str("はっぴゃく"),
            other => {
                out.push_str(UNITS[other as usize - 1]);
                out.push_str("ひゃく");
            }
        }
    }
    if tens > 0 {
        if tens > 1 {
            out.push_str(UNITS[tens as usize - 1]);
        }
        out.push_str("じゅう");
    }
    if units > 0 {
        out.push_str(UNITS[units as usize - 1]);
    }
    Some(out)
}

/// The reading of `value` followed by `counter`, or `None` when the number is not readable.
///
/// `counter_reading` is what the analyzer already has for the counter on its own, and is used
/// wherever [`COUNTERS`] records no alternation — so an unrecognised counter still gets correct
/// place values instead of a per-digit recital.
pub fn counted(value: u64, counter: &str, counter_reading: &str) -> Option<String> {
    let entry = COUNTERS.iter().find(|entry| entry.surface == counter);

    if let Some(entry) = entry {
        if let Some((_, whole)) = entry.whole.iter().find(|(at, _)| *at == value) {
            return Some((*whole).to_string());
        }
    }

    let number = cardinal(value)?;
    let Some(entry) = entry else {
        // No alternations recorded. Correct place values, the counter's own reading, nothing
        // invented — `５％` reaches here and stays ごぱーせんと.
        return Some(format!("{number}{counter_reading}"));
    };

    // The environment is chosen by the value's tail: its last digit, or 10 for a value ending in a
    // bare 十, which is what makes 20分 にじゅっぷん follow 10分 じゅっぷん.
    let tail = if value.is_multiple_of(10) && !value.is_multiple_of(100) {
        10
    } else {
        (value % 10) as u8
    };
    let Some(env) = entry.envs.iter().find(|env| env.tail == tail) else {
        return Some(format!("{number}{}", entry.plain));
    };

    let mut number = number;
    if let Some(replacement) = env.number {
        // The counter reshapes the number's last element — いち becomes いっ, じゅう becomes じゅっ.
        let tail_reading = if tail == 10 {
            "じゅう"
        } else {
            UNITS[tail as usize - 1]
        };
        if let Some(stem) = number.strip_suffix(tail_reading) {
            number = format!("{stem}{replacement}");
        } else {
            // The cardinal did not end the way the table expects, so the environment does not
            // apply to this number. Refusing the alternation is safer than splicing blindly.
            return Some(format!("{number}{}", entry.plain));
        }
    }
    Some(format!("{number}{}", env.counter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_values_are_assembled_rather_than_recited() {
        // The defect this exists for: these were さんよん, いちご, にぜろ, いちきゅうろくさん.
        assert_eq!(cardinal(34).as_deref(), Some("さんじゅうよん"));
        assert_eq!(cardinal(15).as_deref(), Some("じゅうご"));
        assert_eq!(cardinal(20).as_deref(), Some("にじゅう"));
        assert_eq!(
            cardinal(1963).as_deref(),
            Some("せんきゅうひゃくろくじゅうさん")
        );
        assert_eq!(cardinal(25).as_deref(), Some("にじゅうご"));
    }

    #[test]
    fn the_irregular_place_values_are_the_well_known_ones() {
        assert_eq!(cardinal(100).as_deref(), Some("ひゃく"));
        assert_eq!(cardinal(300).as_deref(), Some("さんびゃく"));
        assert_eq!(cardinal(600).as_deref(), Some("ろっぴゃく"));
        assert_eq!(cardinal(800).as_deref(), Some("はっぴゃく"));
        assert_eq!(cardinal(1000).as_deref(), Some("せん"));
        assert_eq!(cardinal(3000).as_deref(), Some("さんぜん"));
        assert_eq!(cardinal(8000).as_deref(), Some("はっせん"));
        // 一 is spoken from 万 up and not below it.
        assert_eq!(cardinal(10).as_deref(), Some("じゅう"));
        assert_eq!(cardinal(10_000).as_deref(), Some("いちまん"));
        assert_eq!(cardinal(100_000_000).as_deref(), Some("いちおく"));
        assert_eq!(cardinal(1_000_000_000_000).as_deref(), Some("いっちょう"));
        assert_eq!(cardinal(0).as_deref(), Some("ぜろ"));
    }

    #[test]
    fn a_larger_number_is_built_from_its_groups() {
        assert_eq!(
            cardinal(12_345).as_deref(),
            Some("いちまんにせんさんびゃくよんじゅうご")
        );
        assert_eq!(cardinal(1_000_001).as_deref(), Some("ひゃくまんいち"));
    }

    #[test]
    fn counter_euphony_is_recorded_not_derived() {
        // The report's own cases.
        assert_eq!(
            counted(34, "歳", "さい").as_deref(),
            Some("さんじゅうよんさい")
        );
        assert_eq!(counted(13, "歳", "さい").as_deref(), Some("じゅうさんさい"));
        assert_eq!(counted(20, "分", "ふん").as_deref(), Some("にじゅっぷん"));
        assert_eq!(counted(15, "分", "ふん").as_deref(), Some("じゅうごふん"));
        assert_eq!(
            counted(1963, "年", "ねん").as_deref(),
            Some("せんきゅうひゃくろくじゅうさんねん")
        );
        assert_eq!(counted(25, "人", "にん").as_deref(), Some("にじゅうごにん"));
        assert_eq!(counted(8, "冊", "さつ").as_deref(), Some("はっさつ"));
        assert_eq!(counted(4, "人", "にん").as_deref(), Some("よにん"));
        assert_eq!(counted(8, "時", "じ").as_deref(), Some("はちじ"));
        assert_eq!(counted(10, "日", "にち").as_deref(), Some("とおか"));

        // 週間, added on the evidence recorded beside the table entry.
        //
        // 1 and 2 are what the installed dictionaries state directly; 8 and 10 follow the
        // /s/-initial family the table already carries. 11 is here because it is the case that
        // decides whether the rule is about the final digit — it is, so no extra row is needed.
        assert_eq!(
            counted(1, "週間", "しゅうかん").as_deref(),
            Some("いっしゅうかん")
        );
        assert_eq!(
            counted(2, "週間", "しゅうかん").as_deref(),
            Some("にしゅうかん")
        );
        assert_eq!(
            counted(3, "週間", "しゅうかん").as_deref(),
            Some("さんしゅうかん")
        );
        assert_eq!(
            counted(8, "週間", "しゅうかん").as_deref(),
            Some("はっしゅうかん")
        );
        assert_eq!(
            counted(10, "週間", "しゅうかん").as_deref(),
            Some("じゅっしゅうかん")
        );
        assert_eq!(
            counted(11, "週間", "しゅうかん").as_deref(),
            Some("じゅういっしゅうかん"),
            "the environment is chosen by the final digit, so a longer number needs no new row"
        );

        // **Controls that must not move.** The /s/-initial counters the new entry was modelled on,
        // and the native series a table edit broke once before.
        assert_eq!(counted(1, "歳", "さい").as_deref(), Some("いっさい"));
        assert_eq!(counted(10, "歳", "さい").as_deref(), Some("じゅっさい"));
        assert_eq!(counted(2, "歳", "さい").as_deref(), Some("にさい"));
        assert_eq!(counted(1, "回", "かい").as_deref(), Some("いっかい"));
        assert_eq!(counted(4, "年", "ねん").as_deref(), Some("よねん"));
        assert_eq!(counted(1, "つ", "つ").as_deref(), Some("ひとつ"));
        assert_eq!(counted(10, "つ", "つ").as_deref(), Some("とお"));
    }

    /// Days of the month keep the older 7 and 9 above the irregular run.
    #[test]
    fn the_seventeenth_is_not_read_with_nana() {
        assert_eq!(counted(17, "日", "にち").as_deref(), Some("じゅうしちにち"));
        assert_eq!(counted(19, "日", "にち").as_deref(), Some("じゅうくにち"));
        assert_eq!(
            counted(27, "日", "にち").as_deref(),
            Some("にじゅうしちにち")
        );
        // …while the irregular run below eleven is still consulted first.
        assert_eq!(counted(7, "日", "にち").as_deref(), Some("なのか"));
        assert_eq!(counted(9, "日", "にち").as_deref(), Some("ここのか"));
        assert_eq!(counted(14, "日", "にち").as_deref(), Some("じゅうよっか"));
    }

    /// **Four counters, one environment, four answers** — which is why this is a table.
    #[test]
    fn the_same_number_reshapes_different_counters_differently() {
        assert_eq!(counted(3, "本", "ほん").as_deref(), Some("さんぼん"));
        assert_eq!(counted(3, "分", "ふん").as_deref(), Some("さんぷん"));
        assert_eq!(counted(3, "枚", "まい").as_deref(), Some("さんまい"));
        assert_eq!(counted(3, "階", "かい").as_deref(), Some("さんがい"));
        assert_eq!(counted(3, "匹", "ひき").as_deref(), Some("さんびき"));
    }

    #[test]
    fn wholly_irregular_counts_are_their_own_words() {
        assert_eq!(counted(1, "人", "にん").as_deref(), Some("ひとり"));
        assert_eq!(counted(2, "人", "にん").as_deref(), Some("ふたり"));
        assert_eq!(counted(1, "日", "にち").as_deref(), Some("ついたち"));
        assert_eq!(counted(2, "日", "にち").as_deref(), Some("ふつか"));
        assert_eq!(counted(20, "日", "にち").as_deref(), Some("はつか"));
        assert_eq!(counted(20, "歳", "さい").as_deref(), Some("はたち"));
    }

    /// **The native series is not Sino-Japanese**, and getting this wrong is a visible regression
    /// on some of the most common words in the language.
    #[test]
    fn the_tsu_counter_uses_the_native_numerals() {
        assert_eq!(counted(1, "つ", "つ").as_deref(), Some("ひとつ"));
        assert_eq!(counted(2, "つ", "つ").as_deref(), Some("ふたつ"));
        assert_eq!(counted(3, "つ", "つ").as_deref(), Some("みっつ"));
        assert_eq!(counted(9, "つ", "つ").as_deref(), Some("ここのつ"));
        assert_eq!(counted(10, "つ", "つ").as_deref(), Some("とお"));
        // Above ten the native series runs out and the ordinary counter takes over.
        assert_eq!(counted(11, "つ", "つ").as_deref(), Some("じゅういちつ"));
    }

    /// A number ending in a bare 十 alternates the way 10 does.
    #[test]
    fn a_round_number_follows_ten() {
        assert_eq!(counted(10, "分", "ふん").as_deref(), Some("じゅっぷん"));
        assert_eq!(counted(30, "分", "ふん").as_deref(), Some("さんじゅっぷん"));
        assert_eq!(counted(40, "回", "かい").as_deref(), Some("よんじゅっかい"));
        // …but a round *hundred* does not: 100分 is ひゃくふん, not ひゃくぷん.
        assert_eq!(counted(100, "分", "ふん").as_deref(), Some("ひゃくふん"));
    }

    /// **An unrecognised counter gets place values and nothing else.**
    ///
    /// This is the whole reason the table can be short: being absent from it costs correctness
    /// about euphony, which was never there, and not correctness about the number.
    #[test]
    fn an_unknown_counter_keeps_its_own_reading() {
        assert_eq!(
            counted(5, "％", "ぱーせんと").as_deref(),
            Some("ごぱーせんと")
        );
        assert_eq!(
            counted(34, "メートル", "めーとる").as_deref(),
            Some("さんじゅうよんめーとる")
        );
    }

    #[test]
    fn digits_are_read_in_either_width() {
        assert_eq!(parse_digits("34"), Some(34));
        assert_eq!(parse_digits("３４"), Some(34));
        assert_eq!(parse_digits("3４"), Some(34));
        assert_eq!(parse_digits("0"), Some(0));
    }

    /// **Not every digit string is a quantity**, and these are the ones that are not.
    #[test]
    fn what_is_refused_as_a_number() {
        assert_eq!(parse_digits("007"), None, "a leading zero is an identifier");
        assert_eq!(
            parse_digits("0120"),
            None,
            "a phone number, not one hundred and twenty"
        );
        assert_eq!(
            parse_digits("12345678901234567"),
            None,
            "17 digits is a card number, not a count"
        );
        assert_eq!(parse_digits(""), None);
        assert_eq!(parse_digits("１2a"), None);
        assert_eq!(parse_digits("3.14"), None, "the separator is not a digit");
    }
}
