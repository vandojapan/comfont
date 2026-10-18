-- テキスト本文内Luaの出力直前に追加。textは表示する文字列の変数名に変更してください。
-- 後段のスクリプト制御フィルタへ貼るだけでは、描画済みテキストは変更できません。
local cf = obj.module("compositefont")
local profile = obj.getvalue("描画@comfont", "comfont::プロファイル")
local _, size, _, _, _, _, _, spacing = obj.getfont()
if profile ~= nil then text = cf.decorate_text(text, profile, size, spacing) end
