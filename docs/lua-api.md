# Lua API

`local cf = obj.module("compositefont")`で取得します。APIバージョンは13です。任意引数の`profile`を省略すると`default`を使用します。

## 装飾文字列

- `decorate(text, profile?, base_font_size?, base_char_spacing?)`: 可視文字へフォント・補正を適用し、AviUtl2制御文字列を返します。anm2の現行経路で使います。
- `decorate_layout(text, profile?, base_font_size?, base_char_spacing?, base_font_family?)`: 等幅の先頭フォントについて上端見切れを補正する互換API。第2戻り値は常に0です。

```lua
local cf = obj.module("compositefont")
local font, size, _, _, _, _, _, spacing = obj.getfont()
local text = cf.decorate_layout("漢字 ABC123", "subtitle", size, spacing, font)
obj.mes(text)
```

`obj.mes`の例は標準テキスト内のLua制御スクリプト用です。範囲別リガチャ解除を含むプロファイルは描画専用タグ`<cf_liga0> / <cf_liga>`を生成するため、`render_decorated`を使います。Luaが生成した文字列も明示的に渡せば装飾できます。

decorateは原文の書記素単位で設定を解決し、結合文字の途中へタグを挿入しません。範囲・分類は書記素の先頭文字、候補フォントの字形確認は正規化後の書記素全体を使います。描画APIは一時文字列をNFC正規化してDirectWriteで組み、保存本文を変更しません。

対応する原文の基本書式は`<#>`系の色、`<s>`系のサイズ・フォント・BIS・縁幅、`<@>`系のフォント・装飾と`<@+BIS> / <@-BIS>`です。Bは太字、Iは斜体、Sは取り消し線。原文の指定が属性ごとにプロファイルより優先され、リセット後は適用を再開します。

色は6桁RGBと標準名white / red / yellow / green / aqua / blue / magenta / blackに対応します。未知・不正・未対応タグ、存在しないプロファイル、不正な補正値、制御文字と衝突するフォント名では原文を返します。描画APIの対応範囲はさらに[anm2仕様](anm2-spec.md)の制限を受けます。

## 解決・描画・互換関数

| 関数 | 内容 |
|---|---|
| `resolve(character, profile?, base_font_size?)` | 1 Unicodeスカラー値の設定を取得 |
| `resolve_codepoint(codepoint, profile?, base_font_size?)` | コードポイントで同じ設定を取得 |
| `profile_names()` | プロファイル名一覧 |
| `decode_text(alias_text)` | SDKのalias表現を改行・バックスラッシュへ一度だけ復号 |
| `render_decorated(text, font, size, spacing, line_spacing, color, align, bold, italic, disable_ligatures?)` | 生成済み制御文字列を画像化。プロファイルを再適用しない |
| `render_text(text, profile, font, size, spacing, line_spacing, color, align, bold, italic)` | 旧anm2用の画像化 |
| `profile_name(selection, saved_label?)` | 旧選択IDと保存ラベルから名前を取得。不一致はnil |
| `api_version()` | 13 |

resolveの戻り値はフォント名、サイズ倍率、ベースライン、字送り、垂直倍率、水平倍率、文字種名、ルールIDの8個です。ベースライン・字送りは基準サイズ1に対する比率です。px指定の行を解決するには基準サイズが必要です。空文字・複数文字・存在しない名前はエラーです。

`render_decorated`の末尾の`disable_ligatures`は省略時falseです。trueで描画専用文字列に非結合文字を挿入し、必須結合も抑止します。原文は変更しませんが、接続形・結合記号・カーニング・字間も変わります。旧API 12の呼び出しは従来どおり動作します。プロファイルの範囲別`disable_ligatures`も描画へ引き継ぎます。末尾trueは全範囲を強制解除し、falseまたは省略時は範囲別設定に従います。

描画関数は失敗時nil、成功時`data / width / height`を持つ所有権付きuserdataを返します。`obj.putpixeldata`への同期アップロードが完了するまでuserdataを保持してください。
