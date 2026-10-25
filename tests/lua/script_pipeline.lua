-- Run shipped Lua scripts with a mocked host/module boundary.
local saved = {}
local cache, seen, output
local cf = {}
local version = 16
local selected_profile = "default"
local force_ligatures = false
function cf.api_version() return version end
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
function cf.decorate_text(text, name, size, spacing, force)
    seen = { text = text, name = name, size = size, spacing = spacing, force = force }
    if name == "missing" or name == "empty" or text == "" then return text end
    return "<#red>" .. text
end
obj = {
    id = 42, frame = 1.5, time = 0.05, num = 1, ox = 0, oy = 0, oz = 0,
    module = function(name) assert(name == "compositefont"); return cf end,
    getvalue = function(effect, item)
        if effect == "スクリプト制御" then return saved.control end
        if effect == "描画@comfont" then
            if selected_profile == nil then return nil end
            if item == "comfont::プロファイル" then return selected_profile end
            if item == "comfont::リガチャを強制ON" then return force_ligatures end
            return nil
        end
        assert(effect == "テキスト"); return saved[item]
    end,
    getfont = function() return "Yu Gothic UI", 64, 0, 0, 0, 0, 0, 3 end,
    mes = function(text) output = text end,
    effect = function(name) assert(name == "Comfont native text") end,
}
saved["文字揃え"] = "中央揃え[中]"
saved["文字装飾"] = "標準文字"
saved["フォント"] = "Yu Gothic UI"
saved["サイズ"] = 64
saved["表示速度"] = 0
saved["字間"] = 3

-- The installed effect delegates drawing to the native filter.
assert(loadstring(anm2_source, "@comfont draw effect"))()
assert(output == nil)
assert(not anm2_source:find("putpixeldata", 1, true))
assert(not anm2_source:find("render_decorated", 1, true))

-- Profile/force flags never rewrite saved source or decorate/draw by themselves.
for _, source_text in ipairs({ "UD新ゴ 漢字かなABC", "ଳି", "<?error('must not execute')?>" }) do
    saved["テキスト"] = source_text
    for _, profile in ipairs({ "default", "changed", "missing" }) do
        selected_profile = profile
        for _, force in ipairs({ false, true }) do
            force_ligatures = force
            seen, output = nil, nil
            assert(loadstring(anm2_source, "flag-only draw effect"))()
            assert(saved["テキスト"] == source_text and output == nil and seen == nil)
            assert(obj.getvalue("描画@comfont", "comfont::プロファイル") == profile)
        end
    end
end
selected_profile, force_ligatures = "default", false

-- The PSDToolKit modifier returns decorated text to the host text renderer.
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
        local text = voice.text
        if type(opts.modifier) == "function" then text = opts.modifier(text) end
        object.mes(text)
        object.ox, object.oy, object.oz = voice.x, voice.y, voice.z
    end }
end
assert(loadstring(body, "subtitle"))()
assert(output == "<w0.050000000><#red>" .. voice.text)
assert(seen.text == voice.text and seen.name == "default" and seen.size == 64 and seen.spacing == 3)
assert(cache.text == voice.text and cache.source == encoded)
assert(obj.ox == 5 and obj.oy == 6 and obj.oz == 7)
selected_profile, force_ligatures = "changed", true
assert(loadstring(body, "subtitle"))()
assert(seen.name == "changed" and seen.force == nil) -- The profile flag is preview-only.
selected_profile = nil -- No active draw effect means the subtitle is unchanged.
assert(loadstring(body, "subtitle"))()
assert(output == "<w0.050000000>" .. voice.text)
assert(seen.name == "changed")
selected_profile = "default"
voice = nil
assert(loadstring(body, "subtitle"))()
assert(cache.text == "")
voice = { text = "fallback", time = 0, x = 0, y = 0, z = 0 }
cache = nil
version = 13
assert(loadstring(body, "subtitle"))()
assert(output == "<w0.050000000>fallback" and not cache)
version = 17
seen = nil
assert(loadstring(body, "subtitle API 17 hand-off"))()
assert(output == "<w0.050000000>fallback" and seen == nil)
assert(cache.text == "fallback" and cache.source == encoded)
voice = nil
assert(loadstring(body, "empty subtitle API 17"))()
assert(cache.text == "")
version = 16

-- The pipe only collects explicitly submitted/generated text; it never draws.
local complete = pipe_source
assert(type(complete) == "string")
local function pipe()
    local run = assert(loadstring(complete, "pipe"))
    local ok, error_message = pcall(run)
    assert(ok, error_message)
    return ok
end
assert(not complete:find("--text@", 1, true))
assert(not complete:find("--string@", 1, true))
assert(not complete:find("loadstring", 1, true))
cache = nil
selected_profile = "default"
saved["テキスト"] = "BASE\\nline"
assert(pipe())
assert(cache.text == "BASE\nline" and cache.producer == nil)
saved.control = "current script"
cf.capture_text("processed result", tostring(obj.id), obj.frame, saved["テキスト"], saved.control)
assert(pipe())
assert(cache.text == "processed result" and cache.producer == "スクリプト制御")
saved.control = nil
assert(pipe())
assert(cache.text == "BASE\nline" and cache.producer == nil)
saved["テキスト"] = "changed original"
assert(pipe())
assert(cache.text == "changed original")
cf.capture_text("other object", "43", obj.frame, saved["テキスト"])
assert(pipe())
assert(cache.text == "changed original" and cache.id == tostring(obj.id))
cf.capture_text("other frame", tostring(obj.id), obj.frame + 1, saved["テキスト"])
assert(pipe())
assert(cache.text == "changed original" and cache.frame == obj.frame)
text = "unrelated global"
assert(pipe())
assert(cache.text == "changed original")
text = nil
saved["テキスト"] = "<?error('must never evaluate saved Lua')?>"
cf.capture_text("stale result", tostring(obj.id), obj.frame, saved["テキスト"], "removed producer")
assert(pipe())
assert(cache == nil, cache and ("stale cache remained: " .. tostring(cache.text)))
saved.control = "linked script"
cf.capture_text("linked result", tostring(obj.id), obj.frame, saved["テキスト"], saved.control)
assert(pipe())
assert(cache.text == "linked result")
cf.capture_text("", tostring(obj.id), obj.frame, saved["テキスト"], saved.control)
assert(pipe())
assert(cache.text == "")
version = 14
cache = nil
assert(pipe())
assert(cache == nil)
assert(not pipe_source:find("putpixeldata", 1, true))
assert(not pipe_source:find("render_decorated", 1, true))

-- Manual integration runs before the existing inline Lua's output, not in a
-- later filter. It edits only the generated string and never emits a duplicate.
version = 16
saved["テキスト"] = "<?existing producer?>"
local saved_source = saved["テキスト"]
local function manual(profile, force)
    selected_profile, force_ligatures = profile, force
    seen, output, text = nil, nil, "generated ABC"
    assert(loadstring(inline_snippet, "manual inline integration"))()
    assert(output == nil and saved["テキスト"] == saved_source)
    if profile == nil or version >= 17 then
        assert(text == "generated ABC" and seen == nil)
    else
        assert(seen.name == profile and seen.size == 64 and seen.spacing == 3)
        assert(seen.force == nil) -- Native text cannot override typography features.
        assert(text == (profile == "missing" and "generated ABC" or "<#red>generated ABC"))
    end
    obj.mes(text) -- The user's original output call stays in place.
    assert(output == text)
end
manual("default", false)
manual("default", true)
manual("default", 1)
manual("default", "1")
manual("missing", false)
manual(nil, false) -- No active draw effect: keep the generated text unchanged.

-- API 17 hands the undecorated full output to the native filter while keeping
-- the original mes call. Do not predecorate the first native image: its centre
-- must match an ordinary text input with the same full output.
version = 17
cache = nil
assert(not inline_snippet:find("<" .. "?", 1, true))
assert(not inline_snippet:find("?" .. ">", 1, true))
manual("default", false)
assert(cache.text == "generated ABC" and cache.source == saved_source)
assert(cache.id == tostring(obj.id) and cache.frame == obj.frame)
assert(cache.producer == nil)
assert(pipe()) -- The collection-only pipe must preserve explicit inline captures.
assert(cache.text == "generated ABC")
text = ""
assert(loadstring(inline_snippet, "empty inline capture"))()
assert(cache.text == "")
saved["テキスト"] = nil -- obj.load("text") producer without a standard text input.
text = "standalone producer"
assert(loadstring(inline_snippet, "non-text producer"))()
assert(text == "<#red>standalone producer")
assert(cache.text == "") -- Never invent a source key for another input type.
saved["テキスト"] = "prefix<?existing producer?>"
text = "partial"
assert(loadstring(inline_snippet, "mixed input fallback"))()
assert(text == "<#red>partial" and cache.text == "")

-- A script-control filter submits the complete replacement, without drawing
-- or requiring Lua in the standard text input. The native filter validates the
-- producer before adopting this submission.
version = 18
saved["テキスト"] = "あABC\\n1️⃣ 🇯🇵 👩‍💻"
saved.control = control_snippet
local control = assert(loadstring(control_snippet, "script control integration"))
cache, seen, output = nil, nil, nil
assert(pcall(control))
assert(cache.text == "あABC\n1️⃣ 🇯🇵 👩‍💻")
assert(cache.source == saved["テキスト"] and cache.producer_source == saved.control)
assert(cache.producer == "スクリプト制御")
assert(cache.id == tostring(obj.id) and cache.frame == obj.frame)
assert(seen == nil and output == nil)
assert(pipe())
assert(cache.producer_source == saved.control)
-- A generic script-control snippet must not overwrite a subtitle captured by
-- inline Lua with the saved Lua program itself, even when the subtitle is empty.
for _, subtitle in ipairs({ "あABC 1️⃣ 🇯🇵 👩‍💻", "" }) do
    saved["テキスト"] = "<?require('PSDToolKit').mes({id='L2'}, obj)?>"
    cf.capture_text(subtitle, tostring(obj.id), obj.frame, saved["テキスト"])
    assert(pcall(control))
    assert(cache.text == subtitle and cache.producer == nil)
end
cache = nil
assert(pcall(control)) -- Unlinked inline Lua is not a generated display string.
assert(cache == nil)
saved["テキスト"] = ""
assert(pcall(control))
assert(cache.text == "")
saved.control = nil -- No provenance: do not manufacture a producer identity.
cache = nil
assert(pcall(control))
assert(cache == nil)
version = 17
assert(not pcall(control)) -- Never silently use an unsupported core.

-- The common PSDToolKit call prints its own text, without an explicit hand-off.
-- Test the documented replacement verbatim so the user can paste it as-is.
version = 18
voice = { text = "あABC 1️⃣ 🇯🇵 👩‍💻", time = 0, x = 5, y = 6, z = 7 }
local original = 'local o={id="L2"}; require("PSDToolKit").mes(o,obj)'
saved["テキスト"] = "<?" .. original .. "?>"
cache, seen, output = nil, nil, nil
assert(loadstring(original, "unlinked standard PSDToolKit subtitle"))()
assert(output == voice.text and cache == nil and seen == nil)
saved["テキスト"] = psd_subtitle_example
local psd_body = assert(psd_subtitle_example:match("^<%?(.*)%?>$"))
local psd_run = assert(loadstring(psd_body, "documented PSDToolKit subtitle"))
cache, seen, output = nil, nil, nil
psd_run()
assert(output == voice.text and cache.text == voice.text and seen == nil)
assert(cache.source == psd_subtitle_example and cache.id == tostring(obj.id) and cache.frame == obj.frame)
assert(cache.producer == nil)
assert(obj.ox == 5 and obj.oy == 6 and obj.oz == 7)
voice = nil
output = nil
psd_run()
assert(cache.text == "" and output == nil) -- No ghost subtitle in an empty frame.
