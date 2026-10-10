use aviutl2::generic::{EditHandle, ObjectHandle};

const PREFIX: &str = "<?local _comfont_v1=\"";
const PROFILE_EFFECT: &str = "合成フォント";

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

pub fn wrap(alias_text: &str) -> Result<String, String> {
    if alias_text.starts_with(PREFIX) {
        unwrap(alias_text)?; // An edited wrapper must not be silently accepted.
        return Ok(alias_text.to_owned());
    }
    if alias_text.contains("<?") {
        return Err("既存のLua制御スクリプトを含む本文は自動変換できません。生成後の文字列をdecorate_layoutへ渡してください。".to_owned());
    }
    if alias_text.contains('\0') {
        return Err("本文にNULが含まれています。".to_owned());
    }
    Ok(format!(
        "{PREFIX}{}\";local text=_comfont_v1:gsub('%x%x',function(h)return string.char(tonumber(h,16))end);text=text:gsub(string.char(92)..'n',string.char(10));local profile=obj.getvalue('{PROFILE_EFFECT}','comfont::プロファイル');if type(profile)=='string' and profile~='' then local m=obj.module('compositefont');local font,size,_,_,_,_,_,spacing=obj.getfont();text=m.decorate_layout(text,profile,size,spacing,font) end;obj.mes(text)?>",
        hex(alias_text)
    ))
}

pub fn unwrap(wrapped: &str) -> Result<String, String> {
    let encoded = wrapped
        .strip_prefix(PREFIX)
        .and_then(|rest| rest.split_once("\";").map(|(encoded, _)| encoded))
        .ok_or_else(|| "この本文には合成フォントの連携スクリプトがありません。".to_owned())?;
    let source = unhex(encoded)?;
    if wrap(&source)? != wrapped {
        return Err("連携スクリプトが手動編集されています。本文の復元は中止しました。".to_owned());
    }
    Ok(source)
}

pub fn update_text(
    handle: &EditHandle,
    object: ObjectHandle,
    effect: &str,
    index: usize,
    item: &str,
    restore: bool,
) -> Result<(), String> {
    if effect != "テキスト" || item != "テキスト" {
        return Err("標準テキストの「テキスト」項目を右クリックして実行してください。".to_owned());
    }
    handle
        .call_edit_section(move |section| {
            let current = section
                .get_object_effect_item(object, "テキスト", index, "テキスト")
                .map_err(|e| e.to_string())?;
            let updated = if restore {
                unwrap(&current)?
            } else {
                wrap(&current)?
            };
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
    fn preserves_alias_bytes_and_is_idempotent() {
        for text in [
            "",
            "日本語😀\\nABC",
            "quotes \" ' \\ ?> ]=]",
            "<s48,Arial>ABC<s>",
            "a\r\nb",
        ] {
            let wrapped = wrap(text).unwrap();
            assert_eq!(unwrap(&wrapped).unwrap(), text);
            assert_eq!(wrap(&wrapped).unwrap(), wrapped);
            assert_eq!(wrapped.matches("?>").count(), 1);
            // AviUtl expands alias \n sequences before parsing Lua.
            assert!(!wrapped.contains("\\n"));
            assert!(!wrapped.contains(['\n', '\r']));
        }
    }
    #[test]
    fn rejects_existing_dynamic_scripts_and_edited_wrappers() {
        assert!(wrap("<?obj.mes('x')?>").is_err());
        let edited = wrap("本文")
            .unwrap()
            .replace("obj.mes(text)", "obj.mes('edited')");
        assert!(unwrap(&edited).is_err());
        assert!(wrap(&edited).is_err());
        assert!(unwrap("<?local _comfont_v1=\"ff\";?>").is_err());
    }
}
