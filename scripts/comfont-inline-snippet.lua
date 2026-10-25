-- comfont API 17対応。標準テキストの本文全体を1つのLuaブロックにします。
-- textは装飾前の表示全文の変数名に変更し、空文字でも毎フレームこの処理を通してください。
-- この直後に既存のobj.mes(text)を1回だけ残します。旧スニペットへの追記ではなく置換用です。
-- 同じオブジェクトに「描画@comfont」を配置し、プロファイルを指定してください。
-- 後段のスクリプト制御フィルタへ貼るだけでは、描画済みテキストは変更できません。
local cf = obj.module("compositefont")
local profile = obj.getvalue("描画@comfont", "comfont::プロファイル")
local _, size, _, _, _, _, _, spacing = obj.getfont()
if profile ~= nil then
    local handed_off = false
    -- 装飾前の全文を渡す。空本文でも毎回実行する。
    -- 後段の描画@comfontが通常文字と絵文字を合成する（API 17以降）。
    if cf.api_version() >= 17 then
        local ok, source = pcall(obj.getvalue, "テキスト", "テキスト")
        if ok and type(source) == "string" then
            local decoded = cf.decode_text(source)
            -- 本文内Luaの区切り文字がコード中へ現れないように比較する。
            local body = decoded:match("^<%?(.*)%?%>$")
            if body and not body:find("<" .. "?", 1, true) and not body:find("?" .. ">", 1, true) then
                handed_off = cf.capture_text(text, tostring(obj.id), obj.frame, source)
            end
        end
    end
    -- aux2へ渡す場合、先行描画は装飾しない。画像中心・文字サイズの二重補正を避ける。
    if not handed_off then text = cf.decorate_text(text, profile, size, spacing) end
end
