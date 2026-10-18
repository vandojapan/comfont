use aviutl2::generic::{EditHandle, ObjectHandle};

const PREFIX: &str = "<?local _comfont_v1=\"";
const PROFILE_EFFECT: &str = "描画@comfont";

// Encode the exact alias value: restoration does not depend on decoding alias
// escapes, and user text can never inject Lua or terminate the <? ?> block.
fn hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn unhex(value: &str) -> Result<String, String> {
    if !value.len().is_multiple_of(2) || !value.is_ascii() {
        return Err("連携スクリプトの本文データが不正です。".to_owned());
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|chunk| {
            u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16)
                .map_err(|_| "連携スクリプトの本文データが不正です。".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes).map_err(|_| "連携スクリプトの本文がUTF-8ではありません。".to_owned())
}

// Reconstruct only for exact validation of old wrappers before restoring them.
// No menu or event handler creates new wrappers.
fn wrap_encoded_source(alias_text: &str) -> String {
    format!(
        "{PREFIX}{}\";local text=_comfont_v1:gsub('%x%x',function(h)return string.char(tonumber(h,16))end);text=text:gsub(string.char(92)..'n',string.char(10));local ok,m=pcall(obj.module,'compositefont');local profile=obj.getvalue('{PROFILE_EFFECT}','comfont::プロファイル');if ok and m and m.api_version()>=16 and profile~=nil then local v=obj.getvalue('{PROFILE_EFFECT}','comfont::リガチャを強制解除');local force=v==true or v==1 or v=='1';local _,size,_,_,_,_,_,spacing=obj.getfont();local applied,result=pcall(m.decorate_text,text,profile,size,spacing,force);if applied and type(result)=='string' then text=result end end;obj.mes(text)?>",
        hex(alias_text)
    )
}

fn legacy_wrapper(alias_text: &str) -> String {
    format!(
        "{PREFIX}{}\";local text=_comfont_v1:gsub('%x%x',function(h)return string.char(tonumber(h,16))end);text=text:gsub(string.char(92)..'n',string.char(10));local profile=obj.getvalue('{PROFILE_EFFECT}','comfont::プロファイル');if type(profile)=='string' and profile~='' then local m=obj.module('compositefont');local font,size,_,_,_,_,_,spacing=obj.getfont();text=m.decorate_layout(text,profile,size,spacing,font) end;obj.mes(text)?>",
        hex(alias_text)
    )
}

pub fn unwrap(wrapped: &str) -> Result<String, String> {
    let encoded = wrapped
        .strip_prefix(PREFIX)
        .and_then(|rest| rest.split_once("\";").map(|(encoded, _)| encoded))
        .ok_or_else(|| "この本文には合成フォントの連携スクリプトがありません。".to_owned())?;
    let source = unhex(encoded)?;
    if wrap_encoded_source(&source) != wrapped && legacy_wrapper(&source) != wrapped {
        return Err("連携スクリプトが手動編集されています。本文の復元は中止しました。".to_owned());
    }
    Ok(source)
}

pub fn restore_text(
    handle: &EditHandle,
    object: ObjectHandle,
    effect: &str,
    index: usize,
    item: &str,
) -> Result<(), String> {
    if effect != "テキスト" || item != "テキスト" {
        return Err("標準テキストの「テキスト」項目を右クリックして実行してください。".to_owned());
    }
    handle
        .call_edit_section(move |section| {
            let current = section
                .get_object_effect_item(object, "テキスト", index, "テキスト")
                .map_err(|e| e.to_string())?;
            let updated = unwrap(&current)?;
            if updated != current {
                section
                    .set_object_effect_item(object, "テキスト", index, "テキスト", &updated)
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_both_known_wrappers_without_changing_original_alias_bytes() {
        for source in [
            "日本語😀\\nABC",
            "<future-control>原文",
            "quotes \" ' \\ ?> ]=]",
            "a\r\nb",
        ] {
            for wrapped in [wrap_encoded_source(source), legacy_wrapper(source)] {
                assert_eq!(unwrap(&wrapped).unwrap(), source);
                assert!(unwrap(&wrapped.replace("obj.mes(text)", "obj.mes('edited')")).is_err());
            }
        }
    }

    #[test]
    fn refuses_plain_text_existing_user_lua_and_malformed_wrappers() {
        for source in [
            "原文",
            "<?obj.mes('original')?>",
            "<?local _comfont_v1=\"ff\";?>",
        ] {
            assert!(unwrap(source).is_err());
        }
        let subtitle = include_str!("../../../scripts/合成フォント字幕.object");
        let source = subtitle
            .split_once("テキスト=")
            .unwrap()
            .1
            .lines()
            .next()
            .unwrap();
        assert!(unwrap(source).is_err());
    }
}
