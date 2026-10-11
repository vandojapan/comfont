-- Execute real anm2 and subtitle Lua with a mocked host/module boundary.
local saved = {}
local cache, seen, rendered, output
local cf = {}
function cf.api_version() return 15 end
function cf.forget_text(id, frame)
    if cache and cache.id == id and cache.frame == frame then cache = nil end
    return true
end
function cf.capture_text(text, id, frame, source, producer_source, producer)
    cache = { text = text, id = id, frame = frame, source = source,
        producer_source = producer_source, producer = producer_source and (producer or "スクリプト制御") }
    return true
end
function cf.generated_text(id, frame, source)
    if cache and cache.id == id and cache.frame == frame and cache.source == source then
        return cache.text, cache.producer, cache.producer_source
    end
end
function cf.decode_text(text)
    return (text:gsub("\\(.)", { n = "\n", ["\\"] = "\\" }))
end
function cf.decorate(text, name, size, spacing)
    seen = { text = text, name = name, size = size, spacing = spacing }
    if name == "missing" or name == "empty" or text == "" then return text end
    return "<#red>" .. text
end
function cf.profile_names() return { "default", "changed", "empty" } end
function cf.render_decorated(text, font, size, spacing, lines, color, align, bold, italic, force)
    rendered = { text = text, align = align, force = force }
    return { data = "pixels", width = 120, height = 40 }
end
obj = {
    id = 42, frame = 1.5, time = 0.05, num = 1, ox = 0, oy = 0, oz = 0,
    module = function(name) assert(name == "compositefont"); return cf end,
    getvalue = function(effect, item)
        if effect == "スクリプト制御" then return saved.control end
        assert(effect == "テキスト"); return saved[item]
    end,
    putpixeldata = function(target, data, width, height, format)
        assert(target == "object" and data == "pixels" and format == "bgra")
        output = { width, height }
    end,
    mes = function(text) output = text end,
}
saved["文字揃え"] = "中央揃え[中]"
saved["文字装飾"] = "標準文字"
saved["フォント"] = "Yu Gothic UI"
saved["サイズ"] = 64
saved["表示速度"] = 0
saved["字間"] = 3
profile, disable_ligatures = "default", false
local function effect()
    seen, rendered, output = nil, nil, nil
    assert(loadstring(legacy_anm2_source, "legacy anm2"))()
    assert(loadstring(anm2_source, "anm2"))()
end
local raw = "<?error('must never evaluate the saved Lua')?>"
saved["テキスト"] = raw
effect()
assert(not seen and not output)
local generated = "କ୍ଷ\n日本語\\n<#blue>ABC"
cf.capture_text(generated, tostring(obj.id), obj.frame, raw)
effect()
assert(seen.text == generated and seen.name == "default" and seen.size == 32 and seen.spacing == 3)
assert(rendered.text == "<#red>" .. generated and output[1] == 120 and obj.cx == 0 and obj.cy == 0)
assert(saved["テキスト"] == raw) -- no writeback or second alias decoding
profile = "changed"
effect()
assert(seen.name == "changed" and seen.text == generated)
for _, field in ipairs({ "id", "frame" }) do
    local old = obj[field]
    obj[field] = old + 1
    effect()
    assert(not seen)
    obj[field] = old
end
saved["テキスト"] = raw .. " "
effect()
assert(not seen)
saved["テキスト"] = raw
cf.capture_text("updated", tostring(obj.id), obj.frame, raw)
effect()
assert(seen.text == "updated")
cf.capture_text("", tostring(obj.id), obj.frame, raw)
effect()
assert(seen.text == "" and rendered.text == "" and output)
profile = "missing"
effect()
assert(not output)
profile, disable_ligatures = "default", true
cf.capture_text(generated, tostring(obj.id), obj.frame, raw)
effect()
assert(rendered.force == true)
disable_ligatures = false
cache = nil
saved["テキスト"] = "static\\nline"
effect()
assert(seen.text == "static\nline")
-- Separate script-control filters may submit the final string too.
saved.control = "current script"
cf.capture_text("after control", tostring(obj.id), obj.frame, saved["テキスト"], saved.control)
effect()
assert(seen.text == "after control")
saved.control = nil -- producer disabled or removed
effect()
assert(seen.text == "static\nline")
saved.control = "changed script" -- same-frame edit without a new submission
effect()
assert(seen.text == "static\nline")
cf.capture_text("new control", tostring(obj.id), obj.frame, saved["テキスト"], saved.control)
effect()
assert(seen.text == "new control")
saved["表示速度"] = 1
effect()
assert(not seen and not output)
saved["表示速度"] = 0
local api_version = cf.api_version
cf.api_version = function() return 13 end
effect()
assert(not seen)
cf.api_version = api_version

-- PSDToolKit normal display remains available; decoration runs in anm2 later.
local encoded = assert(subtitle_alias:match("\nテキスト=(.-)\n文字毎"))
local source = cf.decode_text(encoded)
local body = assert(source:match("^<%?(.*)%?>$"))
assert(subtitle_alias:find("effect.name=描画@comfont", 1, true))
saved["テキスト"] = encoded
local voice = { text = "ଳି ABC", time = 0, x = 5, y = 6, z = 7 }
package.preload["PSDToolKit.VoiceStates"] = function()
    return { get = function(_, id) assert(id == ""); return voice end }
end
package.preload["PSDToolKit"] = function()
    return { mes = function(opts, object)
        if not voice then return end
        object.mes(opts.modifier(voice.text))
        object.ox, object.oy, object.oz = voice.x, voice.y, voice.z
    end }
end
assert(loadstring(body, "subtitle"))()
assert(output == "<w0.050000000>" .. voice.text)
assert(cache.text == voice.text and cache.source == encoded)
assert(obj.ox == 5 and obj.oy == 6 and obj.oz == 7)
profile = "changed"
effect()
assert(seen.text == voice.text and seen.name == "changed" and output[1] == 120)
assert(obj.ox == 5 and obj.oy == 6 and obj.oz == 7)
voice = nil
assert(loadstring(body, "subtitle"))()
assert(cache.text == "")
effect()
assert(seen.text == "" and rendered.text == "" and output)
voice = { text = "fallback", time = 0, x = 0, y = 0, z = 0 }
cache = nil
cf.api_version = function() return 13 end
assert(loadstring(body, "subtitle"))()
assert(output == "<w0.050000000>fallback" and not cache)

-- Pipe has no forms and never evaluates the producer's Lua.
obj.module = function(name) assert(name == "compositefont"); return cf end
cf.api_version = api_version
profile = "default"
local function pipe() return pcall(assert(loadstring(pipe_source, "pipe"))) end
assert(not pipe_source:find("--text@", 1, true))
assert(not pipe_source:find("--string@", 1, true))
assert(not pipe_source:find("loadstring", 1, true))
cache = nil
saved["テキスト"] = "BASE\\nline"
assert(pipe())
assert(cache.text == "BASE\nline" and cache.producer == nil)
effect()
assert(seen.text == "BASE\nline")
-- Only a submission matched to this object/frame/source may take priority.
saved.control = "producer script"
cf.capture_text("processed result", tostring(obj.id), obj.frame, saved["テキスト"], saved.control)
assert(pipe())
effect()
assert(seen.text == "processed result" and cache.producer == "スクリプト制御")
saved.control = nil
assert(pipe())
effect()
assert(seen.text == "BASE\nline" and cache.producer == nil)
saved["テキスト"] = "changed original"
assert(pipe())
assert(cache.text == "changed original")
cf.capture_text("other object", "43", obj.frame, saved["テキスト"])
assert(pipe())
assert(cache.text == "changed original" and cache.id == tostring(obj.id))
cf.capture_text("other frame", tostring(obj.id), obj.frame + 1, saved["テキスト"])
assert(pipe())
assert(cache.text == "changed original" and cache.frame == obj.frame)
-- A global named text is not proven to belong to the current object.
text = "unrelated global"
assert(pipe())
assert(cache.text == "changed original")
text = nil
saved["テキスト"] = raw
saved.control = nil
cf.capture_text("stale result", tostring(obj.id), obj.frame, raw, "removed producer")
assert(pipe())
assert(cache == nil) -- never evaluate unlinked saved Lua
cf.capture_text("linked result", tostring(obj.id), obj.frame, raw)
assert(pipe())
assert(cache.text == "linked result")
cf.capture_text("", tostring(obj.id), obj.frame, raw)
assert(pipe())
effect()
assert(seen.text == "" and rendered.text == "" and output)
profile = "empty"
cf.capture_text("unstyled result", tostring(obj.id), obj.frame, raw)
assert(pipe())
effect()
assert(rendered.text == "unstyled result")
profile = "missing"
effect()
assert(not output)
local generated_text = cf.generated_text
cf.generated_text = function() error("cache unavailable") end
profile = "empty"
saved["テキスト"] = "ordinary text"
effect()
assert(not output) -- error text is not a registered generated body
cf.generated_text = generated_text
cache = nil
cf.api_version = function() return 14 end
assert(pipe())
assert(cache == nil)
obj.module = function() error("not installed") end
assert(loadstring(body, "subtitle"))()
assert(output == "<w0.050000000>fallback" and not cache)
