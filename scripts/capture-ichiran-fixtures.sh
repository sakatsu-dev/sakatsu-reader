#!/usr/bin/env bash
#
# Regenerates the real `ichiran-cli -f` output that core/tests/fixtures/ichiran
# holds and the golden tests parse. project notes §10 requires the Phase 1 tests run
# against genuine Ichiran output, not hand-written JSON — this is how you refresh
# it (after an Ichiran upgrade, or when adding a case).
#
# Run inside WSL, where ichiran-cli and its PostgreSQL live:
#   wsl -e bash scripts/capture-ichiran-fixtures.sh
#
# Each case below pins a specific rule from project notes §4/§5. Keep the comment
# with the case; a fixture whose purpose is unrecorded cannot be maintained.
set -euo pipefail

out="$(cd "$(dirname "$0")/../core/tests/fixtures/ichiran" && pwd)"

capture() {
  local name="$1" text="$2"
  printf 'capturing %-22s %s\n' "$name" "$text"
  local json
  json="$(ichiran-cli -f "$text")"
  if command -v jq >/dev/null 2>&1; then
    printf '%s' "$json" | jq --sort-keys . >"$out/$name.json"
  else
    printf '%s\n' "$json" >"$out/$name.json"
  fi
}

# §10's four gate sentences.
capture sensei-nasakenai   '先生ってほんと…情けないですね'
capture kimochi-tsutawatte '気持ちが伝わってきます'
capture kore-o-ki-ni       'これを機に言わせてもらいますが'
capture tabesaserareteita  '食べさせられていた'

# §4's individual traps, isolated so a failure points at one rule.
capture iwasete            '言わせて'        # competing via branches → no phantom Potential
capture kimochi            '気持ち'          # own gloss wins over a speculative conj
capture kuru               '来る'            # 【クる】 reading needs hiragana normalization
capture tabenakatta        '食べなかった'    # the neg flag on prop (fml shows up elsewhere)
capture mizu-o-nomu        '水を飲む'        # the simple baseline

# An ambiguous surface: とり yields eight competing entries wrapped in
# `alternative`, an entry shape with no text of its own. Ordinary sentences hit
# this constantly, so it is a parse requirement, not an edge case.
capture niwatori           'にはとりが鳴く'

# Particles as Ichiran actually returns them — には/では/何も arrive as single
# tokens — plus a 、 that Ichiran normalizes to ", " and so cannot be grounded.
capture kare-niwa          '彼には時間がなかったので、家では何もしなかった'

# A tie: 晴々 comes back as せいせい and はればれ with the *same* score of 65, which is
# Ichiran saying it cannot choose. Which one the array happens to list first is arbitrary,
# and here it is the wrong one — this sentence wants はればれ. The only fixture where the
# embedded parser is allowed to overrule the sidecar.
capture harebare           '晴々とした気分'

# Pattern-matcher cases. なければならない spans two tokens and is matched on the
# first one's *conjugation* (negative provisional), not its spelling; ことができる
# arrives merged into a single token, which is why match rules are surface-verified.
capture ikanakereba        '行かなければならない'
capture miru-koto          '見ることができる'

# ・ comes back as a single ASCII space, not as itself. The whitespace skip in
# to_analysis_with used to drop that segment outright, leaving the separator covered by no
# token: 26 of them across 17 of 259 UD_Japanese-GSD sentences, visible to the reader and
# selectable by nobody. Both lines are UD_Japanese-GSD (CC BY-SA), not composed here.
capture nakaguro           '景観・社会・文化・報道'

# The control for that repair: a genuine space between two words must still produce no token,
# or the fix trades a missing separator for a column of blanks down the reader.
capture interior-space     '映像もYou Tubeから削除しました'

# Contracted auxiliaries. Ichiran names their parts in the *expanded* form the text does not
# spell — ほどけてる is ほどけて + いる, ついちゃった is ついて + ちゃった — so these pin that the
# decomposition is kept for the grammar while only real slices of the source are selectable
# (`component_selection.rs`). Both are Tatoeba sentences bundled by Jitendex (CC BY-SA 4.0),
# chosen by their English: "Your tie has come undone." / "My hair got kinky." The second also
# arrives as six alternatives tied at 72.
capture hodoketeru         'ネクタイがほどけてるよ。'
capture tsuichatta         '寝癖がついちゃった。'

# Contraction recovery (`lexical::repair`). Ichiran's own segmentation splits these, and none is in
# its first five segmentations, so the repair is local and every case below pins one decision.
# The sentences are Tatoeba, bundled by Jitendex (CC BY-SA 4.0), chosen by their English.
#   machigattoru  "You're wrong from the first letter!!" -- 間 | 違っとる, the second half
#                 unanswered: the positive ～ておる case.
#   yattoru       "H-Hey! What're you up to!!" -- やっと | るかぁぁ: no token ends in a tail, so
#                 nothing is investigated. The advice would not be a union of whole tokens anyway.
#   yomitoranakya "Read between the lines." -- one token Ichiran answers itself: not investigated.
#   okonnakya     the word as it appears in the reported capture xnviewmp_bu75dRV6Zb, not the
#                 passage -- 怒 | ん | なきゃ: the positive nasalised negative conditional.
capture machigattoru       '頭文字から間違っとるわい！！'
capture yattoru            'こ、こら！ナニやっとるかぁぁ！！'
capture yomitoranakya      '本当の所を読みとらなきゃ。'
capture okonnakya          '怒んなきゃ'

# The tie-break asks about the word in its sentence (`ichiran::to_analysis`). Heteronym-fixture
# sentences (Tatoeba via Jitendex, CC BY-SA 4.0), each filed under the reading it has there:
#   nagekata  "The boy knows how to throw a curve." -- 方 ties ほう/かた; in context かた.
#   zengakki  "I had a good report last term."      -- 前 ties まえ/ぜん; in context ぜん.
#   ooguchi   "Great talkers are little doers."     -- 大 ties; no token at 大 in context, so the
#             bare word answers おお. The fallback's control.
capture nagekata           'その少年はカーブの投げ方を知っている。'
capture zengakki           '前学期は成績がよかった。'
capture ooguchi            '大口叩きは何もできない。'

# Regression controls for the provisional contextual tie-break: ties the bare word got right and
# the sentence gets wrong. Heteronym-fixture sentences (Tatoeba via Jitendex, CC BY-SA 4.0):
#   kanki         "She was wearing a thick coat against the chill." -- filed under 寒気【かんき】
#   kin-shinkuro  "The 'gold' yearned for by the Japanese synchronized swimming world was not
#                 quite reached." -- filed under 金【きん】
#   kin-kagayaku  "All that glitters is not gold." -- filed under 金【きん】
capture kanki              '彼女は寒気を防ぐために厚いコートを着ていた。'
capture kin-shinkuro       '日本シンクロ界の悲願である金には、あと一歩で届かなかった。'
capture kin-kagayaku       '輝くものすべてが金とは限らない。'

# Grammar parity: what Ichiran hides inside one token, and what an authored rule must find in
# both analyzers' segmentations (`core/tests/grammar_parity.rs`). Tatoeba sentences (CC BY 2.0 FR),
# chosen by reading the English; the number is the Tatoeba sentence id.
#   grammar-sou-ni-nai     #86361    "It isn't likely that she will come." -- そうにない is a part
#                          of the compound 来そうにない, never a token of its own
#   grammar-yone          #10614923 "You are joking, right?" -- よね fused into one particle
#   grammar-wane           #171741   "It is very cold today, isn't it?" -- わね fused
#   grammar-yona           #13074558 "Japanese is difficult." -- よな fused
#   grammar-noka           #10733588 "Does your arm hurt?" -- のか fused
#   grammar-nano           #81561    "Is that true?" -- なの fused
#   grammar-nandesu        #10006250 "I'm anemic." -- なんです as one explanatory auxiliary
#   grammar-nanda-what     #347294   "What's this?" -- control: the pronoun なん, not explanation
#   grammar-dakedo-medial  #11483472 "Snow is pretty, but it's cold." -- だけど joining clauses
#   grammar-dakedo-initial #12486177 "However, there's a problem." -- だけど opening a sentence
#   grammar-demo-initial   #202115   "But they are all people." -- でも opening a sentence
#   grammar-demo-also      #85538    "She's also a beauty." -- control: で + も, not "but"
#   grammar-dokoka         #201037   "Let's meet up somewhere."
#   grammar-nande          #4789     "Why do you ask?"
#   grammar-nanka          #1490034  "I hate snow!" -- なんか downplaying what precedes it
#   grammar-kyuu-ni        #2250894  "He quit without notice." -- adverbial に
#   grammar-kke            #11831595 "What month is it now?" -- だっけ fused
capture grammar-sou-ni-nai     '彼女は来そうにない。'
capture grammar-yone           '冗談だよね？'
capture grammar-wane           '今日はとても寒いわね。'
capture grammar-yona           '日本語って難しいよな。'
capture grammar-noka           '腕が痛いのか？'
capture grammar-nano           '本当なの？'
capture grammar-nandesu        '貧血なんです。'
capture grammar-nanda-what     'これはなんだ？'
capture grammar-dakedo-medial  '雪はきれいだけど、寒い。'
capture grammar-dakedo-initial 'だけど、問題があります。'
capture grammar-demo-initial   'でも、みんな人間です。'
capture grammar-demo-also      '美人でもある。'
capture grammar-dokoka         'どこかで会いましょう。'
capture grammar-nande          'なんで聞くの？'
capture grammar-nanka          '雪なんか嫌いだ！'
capture grammar-kyuu-ni        '彼は急にやめた。'
capture grammar-kke            '今何月だっけ？'

# Grammar parity, second batch: 〜そうだ after a stem, adverbs with に, words the built-in analyzer
# splits into particle-shaped pieces, and rules that must refuse look-alikes. Tatoeba (CC BY 2.0 FR):
#   grammar-sou-verb       #183185   "I think I'm going to faint." -- verb stem + そう
#   grammar-sou-adjective  #9478098  "You look busy." -- adjective stem + そう
#   grammar-sou-sleepy     #3662503  "You look sleepy." -- adjective stem + そう + だ
#   grammar-sou-desu       #2171510  "That's right." -- control: そう with no stem
#   grammar-sou-hontou     #9524715  "You can say that again." -- control: そう after an adverb
#   grammar-sou-mo-nai     #12156144 "I don't think I can." -- そうもない, not 〜そうだ
#   grammar-hontou-ni      #10976114 "I was really tired." -- に making an adverb of 本当
#   grammar-issho-ni       #10550166 "Let's sing together." -- に making an adverb of 一緒
#   grammar-shizuka-ni     #103147   "He walked quietly." -- the copula's に after a な-adjective
#   grammar-toka           #1619851  "How about tomorrow night?" -- one とか
#   grammar-nantoka        #10580672 "We'll manage somehow." -- one word, not なん + と + か
#   grammar-moshikashite   #8838408  "Are you Tom by any chance?" -- one word, not もし + か + して
#   grammar-tokuni         #172252   "It is especially cold this morning." -- one word
#   grammar-tari-tomo      #190255   "Do not stay away from English, even for a day."
#   grammar-futari-tomo    #8579928  "Both of us ought to do that." -- control: ふたり + とも
#   grammar-docchi-ga      #3643242  "Which do you prefer, dogs or cats?"
#   grammar-docchi-mo      #10156114 "They're both good." -- control: どっちも is "both"
#   grammar-soreni-comma   #205709   "And, we get each other's company to boot."
#   grammar-soreni-touch   #205634   "Don't touch it." -- control: それ + に
#   grammar-nanka-something #3367124 "Something's wrong, isn't it?" -- なんか as "something"
#   grammar-you-ni         #138220   "Stay thin." -- ように, not a な-adjective before に
capture grammar-sou-verb       '気絶しそうだ。'
capture grammar-sou-adjective  '忙しそうね。'
capture grammar-sou-sleepy     '眠そうだね。'
capture grammar-sou-desu       'そうです。'
capture grammar-sou-hontou     '本当にそうだね。'
capture grammar-sou-mo-nai     'できそうもない。'
capture grammar-hontou-ni      '本当に疲れた。'
capture grammar-issho-ni       '一緒に歌おう。'
capture grammar-shizuka-ni     '彼は静かに歩いた。'
capture grammar-toka           '明日の夜とかどう？'
capture grammar-nantoka        'なんとかなるさ。'
capture grammar-moshikashite   'もしかして、トムじゃない？'
capture grammar-tokuni         '今朝は特に寒い。'
capture grammar-tari-tomo      '一日たりとも英語から離れるな。'
capture grammar-futari-tomo    'ふたりともそうすべきだよ。'
capture grammar-docchi-ga      '犬と猫どっちが好き？'
capture grammar-docchi-mo      'どっちもいいね。'
capture grammar-soreni-comma   'それに、みんなに仲間もできるしね。'
capture grammar-soreni-touch   'それに触るな。'
capture grammar-nanka-something 'なんかおかしくない？'
capture grammar-you-ni         '太らないように。'

# Grammar parity, third batch, from screening the rules on JESC dialogue: a な-adjective's に before
# なる, the なんで of "because", and the look-alikes that must not take another rule's card. Tatoeba:
#   grammar-na-naru        #176705   "The crowd calmed down." -- 静か + に + なる, "become quiet"
#   grammar-noun-naru      #91184    "She became a doctor." -- control: a noun before に
#   grammar-nande-because  #12662593 "I'm not hungry. I just had breakfast." -- なんで is なので
#   grammar-nande-after-word #76323  "I'm not getting involved in this." -- なんで after a word
#   grammar-sonnani        #8612406  "Don't get so angry." -- control: そんなに is no な-adjective
#   grammar-kanji-nantoka  #139686   "I managed to get in." -- 何とか is one word
#   grammar-ikurademo      #1484936  "Tom had plenty of chances to apologize, but he didn't." -- one word
#   grammar-haa            #11178798 "Huh, I wanted to send a reply..." -- the sigh, split as は / ぁ
#   grammar-konna-fuu-ni   #9447696  "Do it this way." -- control: 風 is a noun, "manner"
#   grammar-you-ni-kanji   #169456   "See to it you do not leave your umbrella behind." -- ように in kanji
capture grammar-na-naru        '群衆は静かになった。'
capture grammar-noun-naru      '彼女は医者になった。'
capture grammar-nande-because  'お腹は空いてないです。朝ごはん食べたばっかりなんで。'
capture grammar-nande-after-word '俺この件に関してノータッチなんで。'
capture grammar-sonnani        'そんなに怒るなよ。'
capture grammar-kanji-nantoka  '何とか入れた。'
capture grammar-ikurademo      'トムは謝るチャンスがいくらでもあったのに、謝らなかった。'
capture grammar-haa            'はぁ。返事が送りたかったんですが、送れませんでした。'
capture grammar-konna-fuu-ni   'こんな風にやってね。'
capture grammar-you-ni-kanji   '傘を忘れない様にしなさい。'

printf 'done → %s\n' "$out"
