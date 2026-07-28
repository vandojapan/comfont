# compositefont script module API v8

`compositefont.mod2`はAviUtl2の制御スクリプトから`compositefont`として参照する。

## `compositefont.api_version()`

戻り値は整数のAPIバージョン。現在は`8`。

## `compositefont.resolve(character, profile?, base_font_size?)`

UTF-8文字列として渡した1個のUnicodeスカラー値を解決する。`profile`を省略した場合は
`"default"`を使用する。空文字、複数のUnicodeスカラー値、不明なプロファイルはエラーになる。

```lua
local font_family, size_ratio, baseline_shift_em, tracking_adjust_em,
      vertical_scale_ratio, horizontal_scale_ratio, category, rule_id =
    compositefont.resolve(character, "subtitle", 64)
```

戻り値は次の8個の多値。末尾2値は診断用なので、通常の適用処理では無視してよい。

1. `font_family: string` — 適用するフォントファミリー。空文字列なら現在のフォントを維持
2. `size_ratio: number` — 基準フォントサイズに掛ける倍率。`1.0`なら変更なし
3. `baseline_shift_em: number` — 基準フォントサイズを1emとするベースライン移動量
4. `tracking_adjust_em: number` — 基準フォントサイズを1emとする字送り補正量
5. `vertical_scale_ratio: number` — Illustratorの垂直比率に相当する倍率。`1.0`なら変形なし
6. `horizontal_scale_ratio: number` — Illustratorの水平比率に相当する倍率。`1.0`なら変形なし
7. `category: string` — 後述する7分類の名前
8. `rule_id: string` — 通常はカテゴリ名。代替候補を使った場合は`category:fallback:N`

第3引数`base_font_size`はpx指定の行を倍率へ換算するために使用する。省略時にpx指定の行が
選ばれた場合は、フォントと垂直・水平比率だけを返し、サイズ・ベースライン・字送りは中立値になる。

`character`は書記素クラスタではなくUnicodeスカラー値単位である。例えば異体字セレクタや
結合文字を含む文字列は複数回に分けて解決する。

## `compositefont.resolve_codepoint(codepoint, profile?, base_font_size?)`

`resolve`と同じ結果を返すが、第1引数にUnicodeコードポイントの整数値を取る。Lua側で
UTF-8文字列をUnicodeスカラー値単位に分割できない場合の入口として使用する。サロゲートや
`U+10FFFF`より大きい値など、Unicodeスカラー値でない整数はエラーになる。

```lua
local font, scale, shift, tracking, vertical_scale, horizontal_scale =
    compositefont.resolve_codepoint(0x6F22, "subtitle", 64) -- 漢
```

## `compositefont.decorate(text, profile?, base_font_size?, base_char_spacing?)`

`decorate`は現在のSDK向け互換バックエンドである。内部ではホスト非依存の文字runを先に解決し、
その後で制御文字へ変換する。将来のネイティブSDK経路とプロファイル形式を共有する設計については
[テキスト適用バックエンド](render-backends.md)を参照する。

プレーンテキストを文字分類ごとのrunへまとめ、AviUtl2の制御文字へ展開した文字列を返す。
`profile`を省略した場合は`"default"`を使用する。

`base_font_size`と`base_char_spacing`を渡すと、em単位またはpx単位のサイズ、ベースライン、
字送りをAviUtl2の値へ換算する。`base_font_size`を取得できない場合、px指定行の3値は変更せず、
フォントと垂直・水平比率だけを展開する。

```lua
local decorated = compositefont.decorate(text, "subtitle", 100, 0)
```

px指定の行は`metric_unit: "px"`と絶対値を保存する。垂直・水平比率はpx指定時も倍率のまま保持する。

```json
"metric_unit": "px",
"size_px": 48.0,
"baseline_shift_px": 3.0,
"tracking_adjust_px": -2.0
```

初期実装はPSDToolKit2の字幕表示を壊さないことを優先し、入力にASCIIの`<`が1個でもあれば
入力を一切変更せず返す。これには既存の制御文字、ルビ、コメント、スクリプト、未知のタグ、
閉じていない`<`が含まれる。その字幕では合成フォントが無効になるが、元の表示は維持される。
生成結果にも`<`が含まれるため、同じ結果へ再度`decorate`を適用しても二重展開されない。

不明なプロファイル、使用中の不正な補正値、タグ区切りと衝突するフォント名についても
エラーを送出せず、入力をそのまま返す。これはPSDToolKit2が`modifier`の例外を字幕上へ
エラー文字列として表示するためである。

PSDToolKit2の`字幕表示`では、設定スクリプトの`o`へ次の`modifier`を追加する。

```lua
modifier = function(text)
    local compositefont = obj.module("compositefont")
    local _, size, _, _, _, _, _, spacing = obj.getfont()
    return compositefont.decorate(text, "subtitle", size, spacing)
end,
```

既存の`modifier`が色などのタグを追加する場合は、プレーンテキストの加工を終えた後、
タグを追加する前に`decorate`を呼ぶ。

通常の`字幕表示`を直接編集せずに利用する場合は、同梱する`合成フォント字幕.object`を
追加する。この専用オブジェクトはPSDToolKit2の`mes()`を呼び出し、`セリフ準備@PSDToolKit`の
テキストへ上記の`modifier`を適用する。設定方法は[合成フォント字幕オブジェクト](subtitle-object.md)を参照する。

PSDToolKit2を使わない任意の本文には、同梱する`合成フォントテキスト.object`を追加するか、
標準テキストオブジェクトのLua制御文字から直接`decorate`を呼び出せる。設定方法とコード例は
[合成フォントテキストオブジェクト](text-object.md)を参照する。

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

## 文字種ごとのフォントフォールバック

各文字種はメイン設定に加えて、`western_fallbacks`、`hiragana_fallbacks`、
`katakana_fallbacks`、`kanji_fallbacks`、`digit_fallbacks`、`symbol_fallbacks`、
`other_fallbacks`という優先順の追加設定を持てる。
module-pluginは各文字について、AviUtl2のFontManagerから取得した`IDWriteFont`のグリフ有無を
確認し、メインフォントに存在しない場合だけ次のフォントを試す。例えばStdをメイン、Pr6を
2行目にすると、Std収録文字はStdのまま、不足文字だけPr6へ切り替わる。各行はフォントだけで
なく、サイズ、ベースライン、字送り、垂直比率、水平比率も独立して持つ。

```json
"kanji": {
  "font_family": "A-OTF Gothic Std",
  "size_ratio": 1.0,
  "baseline_shift_em": 0.0,
  "tracking_adjust_em": 0.0,
  "vertical_scale_ratio": 1.0,
  "horizontal_scale_ratio": 1.0
},
"kanji_fallbacks": [{
  "font_family": "A-OTF Gothic Pr6",
  "size_ratio": 1.02,
  "baseline_shift_em": 0.0,
  "tracking_adjust_em": 0.0,
  "vertical_scale_ratio": 1.0,
  "horizontal_scale_ratio": 1.0
}]
```

AviUtl2へプライベート登録されたフォントもFontManager経由で判定できる。FontManagerから取得
できない候補があっても後続候補の判定は続け、どの候補も判定できない場合は最初の不明候補を
使用する。

`resolve`の`rule_id`は代替フォントを選んだ場合、`kanji:fallback:1`や
`hiragana:fallback:1`のようになる。

## v8の互換性方針

- `FontAdjustment`へ省略可能なpx単位フィールドを追加した。既存JSONは従来どおり%として読み込む。
- `resolve`と`resolve_codepoint`へ省略可能な`base_font_size`を追加した。戻り値の個数は変更しない。
- 垂直比率と水平比率は単位選択にかかわらず倍率のまま保持する。

## v7の互換性方針

- `CompositeFontProfile`へ漢字以外の6文字種にも省略可能な`*_fallbacks`を追加した。
- 既存JSONの`kanji_fallbacks`と各文字種の先頭設定はそのまま読み込める。
- すべての文字種で、選択された行の補正値一式を`resolve`と`decorate`へ適用する。
- Lua関数の引数と戻り値の個数はv6から変更しない。

## v6の互換性方針

- `CompositeFontProfile`へ省略可能な`kanji_fallbacks`を追加した。既存JSONでは空配列になる。
- v5の`fallback_font_families`も引き続き読み込み、エディターでは独立した漢字行へ変換する。
- 選択された漢字行の補正値一式を`resolve`と`decorate`へ適用する。
- Lua関数の引数と戻り値の個数はv5から変更しない。

## v5の互換性方針

- `FontAdjustment`へ省略可能な`fallback_font_families`を追加した。既存JSONでは空配列になる。
- `resolve`と`decorate`はグリフ単位で漢字フォールバックを適用する。
- 関数の引数と戻り値の個数はv4から変更しない。

## v4の互換性方針

- API v4は`decorate`を追加した。`resolve`と`resolve_codepoint`の引数・戻り値はv3から変更しない。
- `decorate`は字幕向けのfail-open APIであり、不明なプロファイルもエラーにしない。

## v3の互換性方針

- 先頭4個の戻り値の順序と意味はAPI v1から変更しない。
- 垂直比率と水平比率を5、6番目へ追加し、診断値を7、8番目へ移動した。
- API v2利用側で診断値を受け取っている場合は、分割代入の位置を更新する。
- 未指定プロファイルは常に`"default"`として扱う。
- 不明なプロファイルを暗黙に`"default"`へフォールバックさせない。

現時点の組み込み`default`プロファイルは中立設定で、フォントを変更せず、すべての補正値を
無変更にする。実際のルールは次段階のプロファイル読み込み機能から供給する。

API v2ではAPI v1の`"japanese_or_other"`を廃止し、上記の7分類へ拡張した。
