# Lua API

`local cf = obj.module("compositefont")` で取得します。APIバージョンは16。省略したプロファイルは `default` です。

## 標準テキスト向けの装飾

`decorate_text(text, profile?, base_font_size?, base_char_spacing?, force_disable_ligatures?)` は標準テキスト制御文字列を返します。内部専用のリガチャマーカーは取り除き、ZWNJ等を挿入しません。第5引数は互換性のため受理しますが、現行の標準出力には適用しません。プロファイルのリガチャ指定も同様です。

非対応・不正なタグ、既存Lua、タブ等を含む入力は全体をそのまま返します。保存本文を編集するAPIではありません。

既存の本文内Luaで手動連携する場合、出力直前へ次の4行を追加します。`text` は表示する文字列の変数名に置き換え、既存の `obj.mes(text)` は残します。

```lua
local cf = obj.module("compositefont")
local profile = obj.getvalue("描画@comfont", "comfont::プロファイル")
local _, size, _, _, _, _, _, spacing = obj.getfont()
if profile ~= nil then text = cf.decorate_text(text, profile, size, spacing) end
```

同じコードを `Plugin/comfont-snippets/decorate-text.lua` に同梱します。効果が未配置・無効なら装飾しません。後段のスクリプト制御へ貼るだけでは、描画済みの標準本文に反映できません。通常本文へは自動挿入しません。

`decorate(text, profile?, base_font_size?, base_char_spacing?)` は内部マーカー `<cf_liga0>` / `<cf_liga>` を含む文字列を返すため、標準描画へ渡す際は `decorate_text` を使います。

## 生成本文の一時連携

`capture_text(text, object_id, frame, source, producer_source?, producer_effect?)` は生成本文をaux2へ一時登録します。`generated_text(object_id, frame, source)` は照合した本文・生成元効果名・生成元コードを返します。`forget_text(object_id, frame)` は登録を破棄します。収集用であり描画APIではありません。

`source` にはテキスト項目のalias値をそのまま渡し、`text` には表示する本文全体を渡します。空本文も空文字としてフレームごとに登録します。他スクリプトのローカル変数や共有globalは参照しません。

登録値はプロジェクトに保存されず、最大64件・合計4 MiB。本文・原文・生成元コードは各64 KiB、ID・生成元名は各128バイトまでで、古い登録から破棄します。

## その他

| 関数 | 内容 |
|---|---|
| `resolve(character, profile?, base_font_size?)` | 1文字の解決結果 |
| `resolve_codepoint(codepoint, profile?, base_font_size?)` | コードポイントで解決 |
| `profile_names()` | プロファイル名一覧 |
| `decode_text(alias_text)` | alias文字列を一度だけ復号 |
| `decorate_layout(text, profile?, base_font_size?, base_char_spacing?, base_font_family?)` | 等幅先頭フォントの上端補正を行う互換API。第2戻り値は0 |
| `profile_name(selection, saved_label?)` | 旧選択IDから名前を解決 |
| `api_version()` | `16` |

`resolve` 系はフォント名、サイズ倍率、ベースライン、字送り、垂直倍率、水平倍率、分類、ルールIDを返します。px指定の解決には基準サイズが必要です。
