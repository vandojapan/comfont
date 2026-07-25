# compositefont script module API v2

`compositefont.mod2`はAviUtl2の制御スクリプトから`compositefont`として参照する。

## `compositefont.api_version()`

戻り値は整数のAPIバージョン。現在は`2`。

## `compositefont.resolve(character, profile?)`

UTF-8文字列として渡した1個のUnicodeスカラー値を解決する。`profile`を省略した場合は
`"default"`を使用する。空文字、複数のUnicodeスカラー値、不明なプロファイルはエラーになる。

```lua
local font_family, size_ratio, baseline_shift_em, tracking_adjust_em,
      category, rule_id = compositefont.resolve(character, "subtitle")
```

戻り値は次の6個の多値。末尾2値は診断用なので、通常の適用処理では無視してよい。

1. `font_family: string` — 適用するフォントファミリー。空文字列なら現在のフォントを維持
2. `size_ratio: number` — 基準フォントサイズに掛ける倍率。`1.0`なら変更なし
3. `baseline_shift_em: number` — 基準フォントサイズを1emとするベースライン移動量
4. `tracking_adjust_em: number` — 基準フォントサイズを1emとする字送り補正量
5. `category: string` — 後述する7分類の名前
6. `rule_id: string` — 現在はカテゴリ名と同じ値

`character`は書記素クラスタではなくUnicodeスカラー値単位である。例えば異体字セレクタや
結合文字を含む文字列は複数回に分けて解決する。

## `compositefont.resolve_codepoint(codepoint, profile?)`

`resolve`と同じ結果を返すが、第1引数にUnicodeコードポイントの整数値を取る。Lua側で
UTF-8文字列をUnicodeスカラー値単位に分割できない場合の入口として使用する。サロゲートや
`U+10FFFF`より大きい値など、Unicodeスカラー値でない整数はエラーになる。

```lua
local font, scale, shift, tracking =
    compositefont.resolve_codepoint(0x6F22, "subtitle") -- 漢
```

## 文字分類

`category`と`rule_id`は次のいずれかになる。判定は表の上から順に行う。

| 値 | 対象 |
|---|---|
| `"hiragana"` | ひらがな。結合濁点・半濁点を含む |
| `"katakana"` | 全角・半角カタカナ。長音符と半角濁点・半濁点を含む |
| `"kanji"` | CJK統合漢字・互換漢字・部首・画・反復記号など |
| `"digit"` | Unicodeの数値文字。ASCII数字と全角数字を含む |
| `"western"` | 半角ASCIIの英字 |
| `"symbol"` | 句読点・記号・絵文字など、英数字・空白・制御文字ではない文字 |
| `"other"` | 空白、全角英字、上記に含まれない他言語の文字など |

文脈なしで1文字ずつ解決できるよう、共有記号の扱いを固定している。`ー`は`"katakana"`、
`・`は`"symbol"`、`々`は`"kanji"`になる。

## v2の互換性方針

- 先頭4個の戻り値の順序と意味はAPI v1から変更しない。
- 診断値は先頭4個より後ろにのみ追加できる。
- 未指定プロファイルは常に`"default"`として扱う。
- 不明なプロファイルを暗黙に`"default"`へフォールバックさせない。

現時点の組み込み`default`プロファイルは中立設定で、フォントを変更せず、すべての補正値を
無変更にする。実際のルールは次段階のプロファイル読み込み機能から供給する。

API v2ではAPI v1の`"japanese_or_other"`を廃止し、上記の7分類へ拡張した。
