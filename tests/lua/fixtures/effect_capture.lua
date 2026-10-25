-- Internal regression fixture for explicit API 18 effect capture; not distributed.
-- このコードをスクリプト制御の「スクリプト」欄へ貼り付けてください。
local cf = obj.module("compositefont")
assert(cf.api_version() >= 18, "comfont API 18以降へ更新してください")
local producer_effect = "スクリプト制御" -- 複数ある場合は自分の番号（例：スクリプト制御:1）を指定。
local source = obj.getvalue("テキスト", "テキスト")
local producer = obj.getvalue(producer_effect, "スクリプト")
if type(source) ~= "string" or type(producer) ~= "string" then return end

local text = cf.decode_text(source)
-- 本文内Luaの出力は、その生成元の連携に任せます。
-- 合成フォント字幕などが登録済みの本文をLua原文で上書きしません。
if text:find("<?", 1, true) then return end
-- ここでtextを加工するか、生成した表示全文をtextへ代入してください。
-- 例：text = text .. " 1️⃣ 🇯🇵 👩‍💻"

-- 空文字も毎フレーム渡します。描画は後段へ任せ、ここでは文字列だけを登録します。
cf.capture_text(text, tostring(obj.id), obj.frame, source, producer, producer_effect)
