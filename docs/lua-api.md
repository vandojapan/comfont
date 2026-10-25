# Lua API

`local cf = obj.module("compositefont")` で取得します。APIバージョンは18。省略したプロファイルは `default` です。

## スクリプト制御で本文を加工する（推奨）

API 18では、**「テキストオブジェクト → スクリプト制御 → 描画@comfont」** の順で使えます。テキスト欄には通常の本文を置き、文字列の加工をスクリプト制御へまとめます。本文内へLuaを書く必要はありません。通常文字はAviUtl2の標準描画、SVG絵文字とキーキャップは部分合成を使います。「リガチャを強制ON」がONでも、この受け渡しでは全体ラスタライズを呼びません。

1. API 18対応パッケージを導入し、AviUtl2を再起動します。
2. 標準テキストへ「スクリプト制御」を追加し、その後に「描画@comfont」を配置します。
3. `Plugin/comfont-snippets/script-control.lua` の内容を、スクリプト制御の「スクリプト」欄へ貼り付けます。原本は [`scripts/comfont-control-snippet.lua`](../scripts/comfont-control-snippet.lua) です。
4. コメントの位置で `text` を加工するか、生成した表示全文を代入します。空文字も毎フレーム登録してください。「描画@comfont」では保存済みプロファイルを選びます。

```lua
local cf = obj.module("compositefont")
assert(cf.api_version() >= 18, "comfont API 18以降へ更新してください")
local producer_effect = "スクリプト制御"
local source = obj.getvalue("テキスト", "テキスト")
local producer = obj.getvalue(producer_effect, "スクリプト")
if type(source) ~= "string" or type(producer) ~= "string" then return end
local text = cf.decode_text(source)
if text:find("<?", 1, true) then return end
-- ここでtextを加工。例：text = text .. " 1️⃣ 🇯🇵 👩‍💻"
cf.capture_text(text, tostring(obj.id), obj.frame, source, producer, producer_effect)
```

このスクリプト制御では `obj.mes`・`obj.load("text", …)`・`decorate_text` を呼びません。全文を登録すると、後段の「描画@comfont」がプロファイル解決と描画を行います。本文内Lua用の `decorate-text.lua` とは貼り付け場所と役割が異なります。

この汎用スニペットは通常本文用です。本文内Luaを検出した場合は登録を行わず、「合成フォント字幕」などの生成元が登録した本文を維持します。Lua原文を表示全文として登録する旧スニペットは、連携済みの字幕まで通常描画へ戻す原因になるため、貼り付け済みのコードも更新してください。本文内Luaの出力を加工する場合は、生成元の `modifier` などで登録直前に加工します。

生成元の効果名・コード、オブジェクトID・フレーム・保存本文が一致し、生成元が「描画@comfont」より前にある場合だけ採用します。コードを変更・削除した場合や順序が逆の場合は、その登録を使いません。スクリプト制御が複数ある場合は、自分の位置を0始まりの効果名で指定してください。2個目なら `"スクリプト制御:1"` です。最後に登録した全文を描画します。

標準「テキスト」入力が1つで、[絵文字合成の条件](emoji.md#自動svg合成の対象条件)を満たす必要があります。フォント・サイズ・字間はテキスト入力とプロファイルで設定し、`obj.setfont` などのLua側の描画状態は引き継ぎません。生成結果にLua・タブ・未対応タグがある場合や上限を超えた場合は元の表示へ戻ります。縁取り・影・ぼかしなど画像への効果は「描画@comfont」の後へ配置します。

API 18のreleaseビルドで、全文置換、国旗・キーキャップ・ZWJ、水平200%、空本文、後段フィルタを映像プレビュー・AVI出力で確認しました。別の長さの本文へ置換した場合は、静的入力と数pxの位置・輪郭差が出ることがあります。[検証条件と比較結果](validation.md#2026-10-25-スクリプト制御からの全文受け渡しapi-18)を参照してください。

## PSDToolKitの字幕表示との連携

`require("PSDToolKit").mes(o, obj)` だけの標準的な字幕表示は、テキスト欄内のLuaです。PSDToolKitが内部で `obj.mes` を呼ぶため、加工後の本文をcomfontへ登録していません。「描画@comfont」や通常本文向けのスクリプト制御スニペットを追加するだけでは、字幕を自動取得できません。後者はテキスト欄のLua原文を検出すると処理を抜けます。

既存の字幕表示を使う場合は、テキスト欄全体を次のコードへ置き換え、「描画@comfont」を追加します。API 17以降に対応します。`id` は元のキャラクターID／レイヤー指定を維持してください。PSDToolKitの `modifier` で表示直前の本文を登録し、元の本文をそのまま返します。`obj.mes` を別に追加する必要はありません。

<!-- comfont-psdtoolkit-example -->
```lua
<?local o = {
    id = "L2",
}
local cf = obj.module("compositefont")
assert(cf.api_version() >= 17, "comfont API 17以降へ更新してください")
local source = obj.getvalue("テキスト", "テキスト")
local function submit(text)
    cf.capture_text(text, tostring(obj.id), obj.frame, source)
end
-- 字幕がないフレームにも空文字を登録します。
submit("")
o.modifier = function(text)
    submit(text)
    return text
end
require("PSDToolKit").mes(o, obj)
?>
```

この例は表示全文を1回だけ出力する字幕用です。既存の `modifier` がある場合は、その加工が終わった本文を登録してください。テキスト入力の表示速度は0、文字装飾は「標準文字」とし、その他の[絵文字合成の条件](emoji.md#自動svg合成の対象条件)も満たす必要があります。通常文字は標準描画、絵文字だけを部分合成します。同梱の「合成フォント字幕.object」も明示登録を行う別の例です。LuaJITでこのコードの未連携時・受け渡し・空字幕を確認しました。同梱字幕aliasではPSDToolKitのLua実体を使い、検証用字幕データによるプレビュー・AVI出力も確認しています。[確認範囲とスニペットの修正](validation.md#2026-10-26-合成フォント字幕とスクリプト制御の干渉)を参照してください。

## 標準テキスト向けの装飾

`decorate_text(text, profile?, base_font_size?, base_char_spacing?, force_disable_ligatures?)` は標準テキスト制御文字列を返します。内部専用のリガチャマーカーは取り除き、ZWNJ等を挿入しません。第5引数は互換性のため受理しますが、現行の標準出力には適用しません。プロファイルのリガチャ指定も同様です。

非対応・不正なタグ、既存Lua、タブ等を含む入力は全体をそのまま返します。保存本文を編集するAPIではありません。

このAPI自体は文字列の装飾だけを行います。API 17の同梱スニペットは装飾前の全文を収集し、後段の「描画@comfont」で通常文字とSVG絵文字を合成します。**通常文字はAviUtl2が描画し、絵文字だけを画像化します。** 全体ラスタライズは使いません。

### 旧スニペットからの更新

1. API 17対応パッケージを導入し、AviUtl2を再起動します。`comfont.aux2`・`@comfont.anm2`・スニペットは同じパッケージのものを使用してください。
2. 既存の本文内Luaで、旧スニペットの `local cf = obj.module(...)` から `cf.decorate_text(...)` までを、下記のコードで置き換えます。文字列を生成する処理と、最後の `obj.mes(text)` は残します。
3. `text` には装飾前の表示全文を渡します。先に `decorate_text` を呼ぶ処理は取り除き、同じ本文を重複して出力しないようにします。
4. 同じオブジェクトへ「描画@comfont」を配置して、保存済みのプロファイル名を指定します。縁取り・影・ぼかしはその後段へ置きます。

パッケージ内のファイルは `Plugin/comfont-snippets/decorate-text.lua`、リポジトリ内の原本は [`scripts/comfont-inline-snippet.lua`](../scripts/comfont-inline-snippet.lua) です。ファイルを更新するだけでは、既に本文へ貼り付けたコードは更新されません。旧コードへの追記ではなく置換してください。

既存の本文内Luaで連携する場合、出力直前へ次を追加します。`text` は表示する文字列の変数名に置き換え、既存の `obj.mes(text)` は残します。本文欄全体を1つの `<? ... ?>` ブロックにし、この `text` だけで表示全文を出力してください。空文字でも毎フレーム呼び出します。

```lua
local cf = obj.module("compositefont")
local profile = obj.getvalue("描画@comfont", "comfont::プロファイル")
local _, size, _, _, _, _, _, spacing = obj.getfont()
if profile ~= nil then
    local handed_off = false
    if cf.api_version() >= 17 then
        local ok, source = pcall(obj.getvalue, "テキスト", "テキスト")
        if ok and type(source) == "string" then
            local decoded = cf.decode_text(source)
            local body = decoded:match("^<%?(.*)%?%>$")
            if body and not body:find("<" .. "?", 1, true) and not body:find("?" .. ">", 1, true) then
                handed_off = cf.capture_text(text, tostring(obj.id), obj.frame, source)
            end
        end
    end
    if not handed_off then text = cf.decorate_text(text, profile, size, spacing) end
end
```

同じコードを `Plugin/comfont-snippets/decorate-text.lua` に同梱します。既に本文へ貼り付けた旧スニペットは、新しい内容へ置き換えてください。効果が未配置・無効なら装飾しません。後段のスクリプト制御へ貼るだけでは、描画済みの標準本文に反映できません。通常本文へは自動挿入しません。

本文の配置は次の形です。コメントの位置へ上記コードを貼り付けます。空文字になるフレームも、スニペットと `obj.mes(text)` を実行してください。

```lua
<?
local text = "あABC 1️⃣ 🇯🇵 👩‍💻"
-- ここに連携スニペットを貼り付ける
obj.mes(text)
?>
```

スニペット内で区切り文字を分割して比較しているのは、AviUtl2の本文内Luaが途中で終了するのを防ぐためです。文字列中へ区切りを直接書く形に変更しないでください。

標準「テキスト」入力が1つで、文字装飾は「標準文字」、横書き・表示速度0など[SVG合成の条件](emoji.md#自動svg合成の対象条件)が必要です。生成結果に未対応タグやLuaを含めることはできません。複数のLuaブロック、ブロック外の文字、別の `obj.mes` による追加出力、Lua内で変更したフォント・座標などの描画状態は引き継ぎません。サイズ・字間・フォントは標準テキスト入力とプロファイルで設定してください。「リガチャを強制ON」がONでも、この連携では全体描画へ切り替えません。

API 17の受け渡し成功時は、最初の `obj.mes` へ装飾前の本文を渡し、後段のaux2で1回だけ装飾します。先行描画の画像中心やサイズへ装飾が二重に作用することを避けます。旧API、標準テキスト入力がない場合、全文ブロックでない場合は `decorate_text` による文字列装飾を使います。

| 使用条件 | 描画方法 |
|---|---|
| API 17・全文が1つのLuaブロック・収集成功 | aux2で通常文字を標準描画し、対応する絵文字を部分合成 |
| API 16（従来版）、または全文ブロックでない本文 | 従来の文字列装飾。SVG絵文字の部分合成は使わない |
| 標準テキスト入力がない `obj.load("text", …)` | 従来の文字列装飾。下記の非テキストオブジェクト用の例を参照 |
| 「描画@comfont」が未配置・無効 | 生成した本文をそのまま出力 |

`decorate(text, profile?, base_font_size?, base_char_spacing?)` は内部マーカー `<cf_liga0>` / `<cf_liga>` を含む文字列を返すため、標準描画へ渡す際は `decorate_text` を使います。

## Lua描画と後段フィルタの実機確認

AviUtl2 v2.1.14とAPI 17のreleaseビルドで、本文内Luaの収集結果を通常本文と同じ合成経路へ渡す方法を確認しました。キーキャップ・国旗・ZWJ、水平200%、後段の「縁取り」＋「ドロップシャドウ」、「ぼかし」が対象です。検証条件と画素比較は[検証記録](validation.md#2026-10-25-lua生成本文の部分合成api-17)を参照してください。

旧スニペットや `decorate_text` だけではSVG合成へ渡らず、Twitter Color Emojiの `🇯🇵` がJPに分かれ、キーキャップが欠落する場合があります。未連携のLua本文へ「描画@comfont」を付けるだけでも処理しません。

スクリプト制御などが自身でテキスト画像を作る場合は、文字列を `obj.load("text", text)` へ渡す前に同じ連携処理を入れられます。`obj.setfont` で描画条件を設定してから `obj.getfont` を呼び、元の描画処理は残します。これはテキスト以外のメディアオブジェクトでの方法です。ホストの `lua.txt` に従い、標準テキストオブジェクトでは `obj.load("text", …)` を使わず、本文内の `obj.mes` を使います。

```lua
obj.setfont("Yu Gothic UI", 48, 0, 0xffffff, 0, false, false, 0, 0)
local text = "表示する文字列"
local cf = obj.module("compositefont")
local profile = obj.getvalue("描画@comfont", "comfont::プロファイル")
local _, size, _, _, _, _, _, spacing = obj.getfont()
if profile ~= nil then text = cf.decorate_text(text, profile, size, spacing) end
obj.load("text", text, 0, 0, 0)
```

この方法では「スクリプト制御 → 描画@comfont → 縁取り → ドロップシャドウ」の順を実機確認しました。標準テキスト入力がないため、「描画@comfont」は描き直さず、スニペットがプロファイル名を読むための設定として使われます。プロファイル名をAPIへ直接渡す方法では、この効果は不要です。フレーム番号を0～29へ更新する本文と後段フィルタの出力も確認しています。任意のLuaスクリプトの制御フローや、PSDToolKit実体との接続まで保証するものではありません。[検証記録](validation.md#2026-10-25-lua本文スクリプト制御連携スニペット)を参照してください。

## 生成本文の一時連携

`capture_text(text, object_id, frame, source, producer_source?, producer_effect?)` は生成本文をaux2へ一時登録します。`generated_text(object_id, frame, source)` は照合した本文・生成元効果名・生成元コードを返します。`forget_text(object_id, frame)` は登録を破棄します。収集用であり描画APIではありません。

`source` にはテキスト項目のalias値をそのまま渡し、`text` には装飾前の表示本文全体を渡します。空本文も空文字としてフレームごとに登録します。他スクリプトのローカル変数や共有globalは参照しません。API 17では全文が1つのLuaブロックの本文を採用します。API 18では生成元効果・コードを添えた明示登録も採用し、現在の設定と効果順序を照合します。この場合は元のテキストにLuaを含む必要はありません。Luaの再実行や保存本文への書き戻しは行いません。

登録値はプロジェクトに保存されず、最大64件・合計4 MiB。本文・原文・生成元コードは各64 KiB、ID・生成元名は各128バイトまでで、古い登録から破棄します。

同梱の「合成フォント字幕.object」もAPI 17以降では収集結果を同じ経路へ渡し、先行描画の二重装飾を避けます。PSDToolKitのLua実体を使った映像確認は検証用字幕データによる範囲であり、セリフ準備・音声からの一連の操作は未検証です。既存オブジェクトへ新しいaliasを配置しても、貼り付け済みの本文は自動更新しません。

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
