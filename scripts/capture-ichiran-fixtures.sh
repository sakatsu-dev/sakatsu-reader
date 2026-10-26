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

printf 'done → %s\n' "$out"
