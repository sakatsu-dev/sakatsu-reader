//! Native sentences for the JLPT coverage suite.
//!
//! # Where these came from
//!
//! Every sentence here is a **native example sentence**, taken from the Tatoeba-derived
//! examples that Jitendex bundles — the same corpus the app already shows in the
//! vocabulary panel and already credits (CC BY-SA 4.0, `project notes` §8). None of it was
//! written for this test.
//!
//! That matters more than it sounds. The alternative was inventing fifty sentences, and
//! the weakness of that is not the grammar — it is that a sentence I write is a sentence
//! I already believe fires, so the suite would measure my expectations rather than the
//! app's behaviour.
//!
//! # Why picking from a corpus is not circular either
//!
//! Candidates were found by **substring search** on the surface each pattern is written
//! with. The tests then ask whether the **token-aware matcher, with its element
//! constraints**, finds the pattern. Those are different mechanisms — passing the first
//! says nothing about passing the second.
//!
//! Measured before relying on it: the overlap between this corpus and the grammar packs'
//! own example sentences is **1 sentence in 3,320**, and that one is excluded. These are
//! held out, and `jlpt_coverage.rs` has a test that keeps them held out.
//!
//! # Regenerating
//!
//! ```text
//! python scripts/harvest-sentences.py > candidates.txt
//! ```
//!
//! Then read the candidates — each comes with its English translation — and choose. The
//! translation is the evidence that a pattern's surface appearing in a sentence really is
//! that pattern doing its job, which a substring match cannot tell you.

/// A sentence that must be recognised as carrying its pattern.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct Sentence {
    /// The JLPT level, taken from the pattern's own `jlpt` field. The corpus is not
    /// graded and is not asked to be.
    pub level: &'static str,
    pub pattern_id: &'static str,
    pub text: &'static str,
    /// The corpus translation. Not asserted on — it is here so a person reading a failure
    /// can see what the sentence means without leaving the file.
    pub english: &'static str,
    /// Another id that counts as finding this, or `""`.
    ///
    /// Not a way of lowering the bar. The packs list a few constructions at two levels —
    /// てほしい is both `hanabira-n3-te-hoshii` and `hanabira-n4-verb-te-hoshii` — and
    /// whichever wins, the learner is shown the right explanation. Recording that is
    /// describing the data; quietly changing the expected id to whatever fired would be
    /// the thing this exists to avoid.
    pub also_accept: &'static str,
}

/// Ten per level, ten different patterns each, so this measures breadth.
#[allow(dead_code)]
pub const SENTENCES: &[Sentence] = &[
    // ---- N5 ----
    Sentence {
        level: "N5",
        pattern_id: "hanabira-n5-a-soreja-b",
        text: "それじゃ、僕はチキンをもらいましょう。",
        english: "Well then, I'll have chicken.",
        also_accept: "",
    },
    Sentence {
        level: "N5",
        pattern_id: "hanabira-n5-a-to-b-to-docchi",
        text: "デンヴァーとモントリオールとどっちが好きですか。",
        english: "Which do you like better, Denver or Montreal?",
        also_accept: "",
    },
    Sentence {
        level: "N5",
        pattern_id: "hanabira-n5-noun-kurai",
        text: "ええ、ほとんど私と同じくらいの背丈になっていますよ。",
        english: "Yes, he's almost as tall as I am.",
        also_accept: "",
    },
    Sentence {
        level: "N5",
        pattern_id: "hanabira-n5-noun-gurai",
        text: "行って帰って、トータル１週間ぐらいの予定になりそうだね。",
        english: "There and back, in total, it looks like being a schedule of about one week.",
        also_accept: "",
    },
    Sentence {
        level: "N5",
        pattern_id: "hanabira-n5-noun-ni-ikimasu",
        text: "８月１０日からサマースクールに行きます。",
        english: "I am going to summer school from August 10.",
        also_accept: "",
    },
    Sentence {
        level: "N5",
        pattern_id: "hanabira-n5-verb-te-kudasai",
        text: "あら、アンジェラと呼んでください。",
        english: "Oh, please call me Angela.",
        also_accept: "",
    },
    Sentence {
        level: "N5",
        pattern_id: "hanabira-n5-naide-kudasai",
        text: "あまりきつくパーマをかけないでください。",
        english: "Don't make the perm too tight, please.",
        also_accept: "",
    },
    Sentence {
        level: "N5",
        pattern_id: "hanabira-n5-mashou",
        text: "そちらの条件を受け入れましょう。",
        english: "We will accept your conditions.",
        also_accept: "",
    },
    Sentence {
        level: "N5",
        pattern_id: "hanabira-n5-keredo",
        text: "ボブは装飾された額にその肖像画を収めたけれど、上下さかさまだった。",
        english: "Bob mounted the portrait in a fancy frame, but it was upside down.",
        also_accept: "",
    },
    Sentence {
        level: "N5",
        pattern_id: "hanabira-n5-dou-shimasu-ka",
        text: "万一それが起これば、君はどうしますか。",
        english: "Should that happen, what will you do?",
        also_accept: "",
    },
    // ---- N4 ----
    Sentence {
        level: "N4",
        pattern_id: "hanabira-n4-verb-te-iru-tokoro",
        text: "彼女はメーキャップを落としているところですよ。",
        english: "She's cleaning off the make-up.",
        also_accept: "",
    },
    Sentence {
        level: "N4",
        pattern_id: "hanabira-n4-verb-te-kudasaru",
        text: "遠路はるばる見送りに来てくださるとはなんとご親切なんでしょう。",
        english: "How kind you are to come all the way to see me off!",
        also_accept: "",
    },
    Sentence {
        level: "N4",
        pattern_id: "hanabira-n4-verb-te-hoshii",
        text: "政府は及び腰と言われないようなしっかりとした対応を見せてほしい。",
        english: "I'd like the government to respond resolutely so as to not end up looking timid.",
        also_accept: "hanabira-n3-te-hoshii",
    },
    Sentence {
        level: "N4",
        pattern_id: "hanabira-n4-nasai",
        text: "帽子を弄り回すのはやめなさい。",
        english: "Please stop playing with your hat.",
        also_accept: "",
    },
    Sentence {
        level: "N4",
        pattern_id: "hanabira-n4-sou-da",
        text: "まもなく雨が降りそうだ。",
        english: "It is going to rain soon.",
        also_accept: "",
    },
    Sentence {
        level: "N4",
        pattern_id: "hanabira-n4-darou",
        text: "そもそもなぜ鳥は移動するのだろうか。",
        english: "Why do birds migrate in the first place?",
        also_accept: "",
    },
    Sentence {
        level: "N4",
        pattern_id: "hanabira-n4-tte-iu",
        text: "「怒ってない。ほら見て、このにこやかな笑顔」「それは作り笑いっていうんですよ」",
        english: "\"I'm not angry. Look, see this smiling face?\" \"That's what they call a forced smile!\"",
        also_accept: "",
    },
    Sentence {
        level: "N4",
        pattern_id: "hanabira-n4-nakya-ikenai",
        text: "すぐに手紙をかかなきゃいけないよ。",
        english: "You've got to write to him quickly.",
        // The curated `nakya` (2026-10-17) takes the いけない after なきゃ and, being in the
        // curated pack, wins the span — the same point, explained once.
        also_accept: "nakya",
    },
    Sentence {
        level: "N4",
        pattern_id: "hanabira-n4-ba-ii",
        text: "５番ゲートへはどう行けばいいのですか。",
        english: "How do I get to Gate 5?",
        also_accept: "",
    },
    Sentence {
        level: "N4",
        pattern_id: "hanabira-n4-mitai-da",
        text: "どうやら食べ過ぎたみたいだ。",
        english: "I'm afraid I've eaten too much.",
        also_accept: "",
    },
    // ---- N3 ----
    Sentence {
        level: "N3",
        pattern_id: "hanabira-n3-tsumari",
        text: "違いはこうだ。つまり、彼のほうが君よりよく働くのだ。",
        english: "The difference is this: he works harder than you.",
        also_accept: "",
    },
    Sentence {
        level: "N3",
        pattern_id: "hanabira-n3-te-hoshii",
        text: "いろいろトラブルもあったけど、雨降って地固まるってことになってほしいね。",
        english: "We've been through a rough patch, but I hope it will have made us stronger.",
        also_accept: "",
    },
    Sentence {
        level: "N3",
        pattern_id: "hanabira-n3-nazenara",
        text: "聞いて驚くなよ！今日から俺はピアノを始めるんだ。なぜなら私は音楽の先生になりたいからだ！",
        english: "Brace yourselves! From today I'm learning the piano. Why? Because I want to become a music teacher!",
        also_accept: "",
    },
    Sentence {
        level: "N3",
        pattern_id: "hanabira-n3-beki-da",
        text: "あなたにはまず最初に尋ねるべきだったでしょうか。",
        english: "Should I have asked you first?",
        also_accept: "",
    },
    Sentence {
        level: "N3",
        pattern_id: "hanabira-n3-mattaku-nai",
        text: "私はソビエト連邦のことはまったく知らない。",
        english: "I don't know about the USSR at all.",
        also_accept: "",
    },
    Sentence {
        level: "N3",
        pattern_id: "hanabira-n3-maru-de-you",
        text: "あなたはまるで幽霊でも見たような顔つきをしている。",
        english: "You look as if you had seen a ghost.",
        also_accept: "",
    },
    Sentence {
        level: "N3",
        pattern_id: "hanabira-n3-metta-ni-nai",
        text: "なにぶんこんな辺ぴな村に住んでいるので、めったに客は来ない。",
        english: "Living as I do in a remote village, I seldom have visitors.",
        also_accept: "",
    },
    Sentence {
        level: "N3",
        pattern_id: "hanabira-n3-wake-da",
        text: "これでこの一画は少なくとも２０匹ニジマスがいるわけだ。",
        english: "So now there are at least 20 rainbow trout in this area.",
        also_accept: "",
    },
    Sentence {
        level: "N3",
        pattern_id: "hanabira-n3-n-datte",
        text: "あの社の新しいコンピューターはアーキテクチャーに問題があって回収騒ぎになっているんだってさ。",
        english: "There was a problem with the architecture of that company's new computer. They're going through a recall frenzy right now.",
        also_accept: "",
    },
    Sentence {
        level: "N3",
        pattern_id: "hanabira-n3-kanarazushimo-towa-kagiranai",
        text: "利益追求に必死のブローカーは、必ずしもルールブックにのっとっているとは限らないのです。",
        english: "Brokers struggling to make a profit don't always play by the book.",
        also_accept: "",
    },
    // ---- N2 ----
    Sentence {
        level: "N2",
        pattern_id: "hanabira-n2-darake",
        text: "この本は間違いだらけだ。",
        english: "This book is full of mistakes.",
        also_accept: "",
    },
    Sentence {
        level: "N2",
        pattern_id: "hanabira-n2-dokoro-ka",
        text: "「あなたは仕事が気に入っていると思います」「いやそれどころか、嫌いです」",
        english: "\"I think you like your job.\" \"On the contrary, I hate it.\"",
        also_accept: "",
    },
    Sentence {
        level: "N2",
        pattern_id: "hanabira-n2-kane-nai",
        text: "規則にうるさいレフェリーは試合を台無しにしかねない。",
        english: "A fussy referee can ruin a bout.",
        also_accept: "",
    },
    Sentence {
        level: "N2",
        pattern_id: "hanabira-n2-tsutsu-aru",
        text: "このことは家が沈み始め、道路が裂けて街灯が途方もない角度に傾きつつあることを意味する。",
        english: "This means that houses are starting to sink, roads are breaking up and lamp-posts are leaning at crazy angles.",
        also_accept: "",
    },
    Sentence {
        level: "N2",
        pattern_id: "hanabira-n2-ppoi",
        text: "何でそんな安っぽいペテンにひっかかるんだ。",
        english: "Why do people get fooled by such cheap schemes?",
        also_accept: "",
    },
    Sentence {
        level: "N2",
        pattern_id: "hanabira-n2-ageku",
        text: "コンピューターを物色して歩いたあげく、デイヴィドより２００ドル安い値段で手に入れた。",
        english: "I shopped around for my computer and ended up paying $200 less than David.",
        also_accept: "",
    },
    Sentence {
        level: "N2",
        pattern_id: "hanabira-n2-kara-niwa",
        text: "私の参謀としてついたからには、危機感を持って任にあたって貰わねば困ります。",
        english: "If you're with me as a staff officer, then I need you to bring a sense of danger with you to your work.",
        also_accept: "",
    },
    Sentence {
        level: "N2",
        pattern_id: "hanabira-n2-dake-atte",
        text: "さすがに偉大な学者だけあって、彼はその問いに容易に答えた。",
        english: "Like the great scholar that he was, he answered the question easily.",
        also_accept: "",
    },
    Sentence {
        level: "N2",
        pattern_id: "hanabira-n2-kotonaku",
        text: "そんな事実にまったく悪びれることなく、千歳は今日も元気に過ごしております。",
        english: "Without flinching from that fact in the slightest, Chitose is spending today as well in fine spirit.",
        also_accept: "",
    },
    Sentence {
        level: "N2",
        pattern_id: "hanabira-n2-ni-kakawarazu",
        text: "晴雨にかかわらず、競技会は挙行されます。",
        english: "Rain or shine, the athletic meet will be held.",
        also_accept: "",
    },
    // ---- N1 ----
    Sentence {
        level: "N1",
        pattern_id: "hanabira-n1-zukume",
        text: "あの黒ずくめの紳士は誰ですか。",
        english: "Who is the gentleman all dressed in black?",
        also_accept: "",
    },
    Sentence {
        level: "N1",
        pattern_id: "hanabira-n1-desura",
        text: "最も強大な帝国ですら崩壊する。",
        english: "Even the mightiest of empires comes to an end.",
        also_accept: "",
    },
    Sentence {
        level: "N1",
        pattern_id: "hanabira-n1-ni-kagiru",
        text: "夏はビールのジョッキ大に限るよ。",
        english: "Nothing beats a big glass of beer in summer.",
        also_accept: "",
    },
    Sentence {
        level: "N1",
        pattern_id: "hanabira-n1-wa-sateoki",
        text: "両親はさておき、誰もその容疑者を弁護しないであろう。",
        english: "Apart from his parents, nobody would defend the suspect.",
        also_accept: "",
    },
    Sentence {
        level: "N1",
        pattern_id: "hanabira-n1-ja-aru-mai-shi",
        text: "禅坊主じゃあるまいし、毎日毎日、一汁一菜のダイエットメニューは勘弁してよ。",
        english: "What? A little soup and celery is all I get? I'm not a Zen monk. I can't survive on an austerity diet like this.",
        also_accept: "",
    },
    Sentence {
        level: "N1",
        pattern_id: "hanabira-n1-wo-hete",
        text: "幾多の紆余曲折を経て条約は締結されるに至った。",
        english: "The treaty has been concluded after many twists and turns.",
        also_accept: "",
    },
    Sentence {
        level: "N1",
        pattern_id: "hanabira-n1-verb-ru-ga-hayai-ka",
        text: "山の麓に日が昇るが早いかただ一人山を登り始めた。",
        english: "The sun had barely risen at the foot of the mountain when she set out alone up the slope.",
        also_accept: "",
    },
    Sentence {
        level: "N1",
        pattern_id: "hanabira-n1-you-mo-nai",
        text: "親がいないとどうしようもないほど悪さをする子供たちもいるものだ。",
        english: "Some children are as bad as they can be while their parents are gone.",
        also_accept: "",
    },
    Sentence {
        level: "N1",
        pattern_id: "hanabira-n1-ni-mo-mashite",
        text: "何ものにもまして私は健康になりたい。",
        english: "Above all, I want to be healthy.",
        also_accept: "",
    },
    Sentence {
        level: "N1",
        pattern_id: "hanabira-n1-tokitara",
        text: "スペイン語の書き言葉は、ほんのいくらかわかるが、スペイン語の話し言葉ときたら、ちんぷんかんぷんだ。",
        english: "I can understand written Spanish just a little, but spoken Spanish is Greek to me.",
        also_accept: "",
    },
];

/// A sentence containing a pattern's *surface* but not its *grammar*.
///
/// These fell out of the same substring search as the positives, which is the point: they
/// are the mistakes a naive matcher makes, found the way a naive matcher finds them. A
/// token-aware rule with constraints must stay silent on every one.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct Negative {
    pub pattern_id: &'static str,
    pub text: &'static str,
    pub english: &'static str,
    /// Why the surface is there but the grammar is not.
    pub why: &'static str,
}

#[allow(dead_code)]
pub const NEGATIVES: &[Negative] = &[
    Negative {
        pattern_id: "hanabira-n1-gurumi",
        text: "その小さな女の子は泣いてクマのぬいぐるみを欲しがった。",
        english: "The little girl was crying for the teddy bear.",
        why: "ぬいぐるみ is one word, a stuffed toy — not Noun + ぐるみ",
    },
    Negative {
        pattern_id: "hanabira-n1-kara-aru",
        text: "テレビはいつからあるの？",
        english: "When did TV appear?",
        why: "いつから + ある, not the Noun からある of quantity",
    },
    Negative {
        pattern_id: "hanabira-n3-moshimo-nara",
        text: "「もしもし、ブラウンさんですか」「はい、そうです」",
        english: "\"Hello, is this Mrs. Brown?\" \"Yes, this is Mrs. Brown.\"",
        why: "もしもし is answering the phone, not もしも〜なら",
    },
    Negative {
        pattern_id: "hanabira-n3-tatoe-temo",
        text: "たとえばあなたの家庭のいざこざを考えてごらんなさい。",
        english: "Take, for instance, your family problems.",
        why: "たとえば is 'for example'; the point needs たとえ〜ても",
    },
    Negative {
        pattern_id: "hanabira-n3-donna-ni-temo",
        text: "私がどんなにうれしいか言葉では伝えられない。",
        english: "Words cannot convey how glad I am.",
        why: "どんなに〜か, an exclamation — there is no ても",
    },
    Negative {
        pattern_id: "hanabira-n5-a-to-b-to-dochira",
        text: "あの〜郵便局はどちらでしょうか。",
        english: "Uh..., where's the post office?",
        why: "どちら asking where, with no A と B と in front of it",
    },
    Negative {
        pattern_id: "hanabira-n4-verb-nai-koto-ga-aru",
        text: "私はなにやら彼に話さなければならないことがある。",
        english: "I have something or other to tell him.",
        why: "ならない + ことがある, not the Verb ない ことがある of occasional non-occurrence",
    },
    Negative {
        pattern_id: "hanabira-n5-i-adjective-ku-shimasu",
        text: "フライトの再確認をお願いします。",
        english: "Flight reconfirmation, please.",
        why: "お願いします — a fixed polite request, not い-adj く します",
    },
];
