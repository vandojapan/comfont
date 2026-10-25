# Lua API

`local cf = obj.module("compositefont")` で取得します。APIバージョンは18。省略したプロファイルは `default` です。

このページは、自分で生成した文字列を扱うAPIの仕様です。テキスト欄内のLuaが出力した文字列を、後段のスクリプトやフィルタが自動取得する機能はありません。

## 標準テキスト向けの装飾

`decorate_text(text, profile?, base_font_size?, base_char_spacing?, force_disable_ligatures?)` は標準テキスト制御文字列を返します。内部専用のリガチャマーカーは取り除き、ZWNJ等を挿入しません。第5引数は互換性のため受理しますが、現行の標準出力には適用しません。プロファイルのリガチャ指定も同様です。

非対応・不正なタグ、既存Lua、タブ等を含む入力は全体をそのまま返します。保存本文を編集するAPIではありません。この関数自体は文字列の装飾だけを行い、SVG絵文字の画像合成は行いません。

`decorate(text, profile?, base_font_size?, base_char_spacing?)` は内部マーカー `<cf_liga0>` / `<cf_liga>` を含む文字列を返すため、標準描画へ渡す際は `decorate_text` を使います。

## 生成本文の一時連携

`capture_text(text, object_id, frame, source, producer_source?, producer_effect?)` は生成本文をaux2へ一時登録します。`generated_text(object_id, frame, source)` は照合した本文・生成元効果名・生成元コードを返します。`forget_text(object_id, frame)` は登録を破棄します。収集用であり描画APIではありません。

`source` にはテキスト項目のalias値をそのまま渡し、`text` には装飾前の表示本文全体を渡します。空本文も空文字としてフレームごとに登録します。他スクリプトのローカル変数や共有globalは参照しません。

API 17では全文が1つのLuaブロックの本文を採用します。API 18では生成元効果・コードを添えた明示登録も採用し、現在の設定と効果順序を照合します。生成元が「描画@comfont」より前にあることが必要です。Luaの再実行や保存本文への書き戻しは行いません。生成結果は[絵文字合成の条件](emoji.md#自動svg合成の対象条件)を満たす必要があります。

登録値はプロジェクトに保存されず、最大64件・合計4 MiB。本文・原文・生成元コードは各64 KiB、ID・生成元名は各128バイトまでで、古い登録から破棄します。

## 同梱字幕オブジェクト

「合成フォント字幕.object」は、PSDToolKitから受け取った本文を明示登録します。`settings.id` にキャラクターIDまたは `L3` などのレイヤー指定を設定し、「描画@comfont」でプロファイルを選びます。任意の本文内Luaの出力を読み取る仕組みではありません。

API 17以降では通常文字の標準描画と絵文字の部分合成へ渡し、先行描画の二重装飾を避けます。この登録本文は「リガチャを強制ON」がONでも全体ラスタライズへ入りません。

PSDToolKitのLua実体を使った映像確認は検証用字幕データによる範囲であり、セリフ準備・音声からの一連の操作は未検証です。既存オブジェクトへ新しいaliasを配置しても、貼り付け済みの本文は自動更新しません。過去の検証条件は[生成本文連携の検証履歴](history/lua-integration-validation.md)を参照してください。

## その他

| 関数 | 内容 |
|---|---|
| `resolve(character, profile?, base_font_size?)` | 1文字の解決結果 |
| `resolve_codepoint(codepoint, profile?, base_font_size?)` | コードポイントで解決 |
| `profile_names()` | プロファイル名一覧 |
| `decode_text(alias_text)` | alias文字列を一度だけ復号 |
| `decorate_layout(text, profile?, base_font_size?, base_char_spacing?, base_font_family?)` | 等幅先頭フォントの上端補正を行う互換API。第2戻り値は0 |
| `profile_name(selection, saved_label?)` | 旧選択IDから名前を解決 |
| `api_version()` | `18` |

`resolve` 系はフォント名、サイズ倍率、ベースライン、字送り、垂直倍率、水平倍率、分類、ルールIDを返します。px指定の解決には基準サイズが必要です。
