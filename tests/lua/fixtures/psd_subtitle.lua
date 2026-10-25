<?local o = {
    id = "L2",
}
local cf = obj.module("compositefont")
assert(cf.api_version() >= 17, "comfont API 17以降へ更新してください")
local source = obj.getvalue("テキスト", "テキスト")
local function submit(text)
    cf.capture_text(text, tostring(obj.id), obj.frame, source)
end
submit("")
o.modifier = function(text)
    submit(text)
    return text
end
require("PSDToolKit").mes(o, obj)
?>
