use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttributeState<T> {
    pub(crate) input_override: bool,
    pub(crate) current_value: Option<T>,
    pub(crate) reset_value: Option<T>,
    pub(crate) object_default: Option<T>,
}

impl<T: Clone> AttributeState<T> {
    fn new(object_default: Option<T>) -> Self {
        Self {
            input_override: false,
            current_value: object_default.clone(),
            reset_value: object_default.clone(),
            object_default,
        }
    }

    fn set(&mut self, value: T) {
        self.input_override = true;
        self.current_value = Some(value);
    }

    fn reset(&mut self) {
        self.input_override = false;
        self.current_value = self.reset_value.clone();
    }

    fn set_reset_value(&mut self, reset_value: Option<T>) {
        self.reset_value = reset_value.clone();
        if !self.input_override {
            self.current_value = reset_value;
        }
    }

    pub(crate) fn profile_allowed(&self) -> bool {
        !self.input_override
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ControlTextDefaults {
    pub(crate) font_size: Option<f64>,
    pub(crate) font_family: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StyleFlags {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) strike: bool,
}

impl StyleFlags {
    pub(crate) fn as_control_letters(self) -> String {
        let mut value = String::with_capacity(3);
        if self.bold {
            value.push('B');
        }
        if self.italic {
            value.push('I');
        }
        if self.strike {
            value.push('S');
        }
        value
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextFormatState {
    pub(crate) text_color: AttributeState<String>,
    pub(crate) decoration_color: AttributeState<String>,
    pub(crate) font_size: AttributeState<f64>,
    pub(crate) at_font_family: AttributeState<String>,
    pub(crate) at_decoration: AttributeState<u8>,
    pub(crate) at_bold: AttributeState<bool>,
    pub(crate) at_italic: AttributeState<bool>,
    pub(crate) at_strike: AttributeState<bool>,
    pub(crate) s_font_family: AttributeState<String>,
    pub(crate) s_bold: AttributeState<bool>,
    pub(crate) s_italic: AttributeState<bool>,
    pub(crate) s_strike: AttributeState<bool>,
    pub(crate) s_outline_size: AttributeState<String>,
    size_restore_expressions: Vec<String>,
    s_style_active: bool,
}

impl TextFormatState {
    fn new(defaults: &ControlTextDefaults) -> Self {
        let at_font_family = AttributeState::new(defaults.font_family.clone());
        let at_bold = AttributeState::new(None);
        let at_italic = AttributeState::new(None);
        let at_strike = AttributeState::new(None);
        Self {
            text_color: AttributeState::new(None),
            decoration_color: AttributeState::new(None),
            font_size: AttributeState::new(defaults.font_size),
            at_font_family: at_font_family.clone(),
            at_decoration: AttributeState::new(None),
            at_bold: at_bold.clone(),
            at_italic: at_italic.clone(),
            at_strike: at_strike.clone(),
            s_font_family: AttributeState {
                reset_value: at_font_family.current_value.clone(),
                ..at_font_family
            },
            s_bold: AttributeState {
                reset_value: at_bold.current_value,
                ..at_bold
            },
            s_italic: AttributeState {
                reset_value: at_italic.current_value,
                ..at_italic
            },
            s_strike: AttributeState {
                reset_value: at_strike.current_value,
                ..at_strike
            },
            s_outline_size: AttributeState::new(None),
            size_restore_expressions: Vec::new(),
            s_style_active: false,
        }
    }

    pub(crate) fn profile_font_allowed(&self) -> bool {
        self.effective_font_state().profile_allowed()
    }

    pub(crate) fn profile_size_allowed(&self) -> bool {
        self.font_size.profile_allowed()
    }

    pub(crate) fn effective_font_family(&self) -> Option<&str> {
        self.effective_font_state().current_value.as_deref()
    }

    pub(crate) fn effective_font_size(&self) -> Option<f64> {
        self.font_size.current_value
    }

    pub(crate) fn size_restore_expressions(&self) -> &[String] {
        &self.size_restore_expressions
    }

    pub(crate) fn s_font_override(&self) -> Option<&str> {
        self.s_font_family
            .input_override
            .then_some(self.s_font_family.current_value.as_deref())
            .flatten()
    }

    pub(crate) fn s_style_override(&self) -> Option<StyleFlags> {
        self.s_style_active.then(|| StyleFlags {
            bold: self.s_bold.current_value.unwrap_or(false),
            italic: self.s_italic.current_value.unwrap_or(false),
            strike: self.s_strike.current_value.unwrap_or(false),
        })
    }

    pub(crate) fn s_outline_override(&self) -> Option<&str> {
        self.s_outline_size
            .input_override
            .then_some(self.s_outline_size.current_value.as_deref())
            .flatten()
    }

    pub(crate) fn at_style_override(&self) -> Option<StyleFlags> {
        (self.at_bold.input_override
            || self.at_italic.input_override
            || self.at_strike.input_override)
            .then(|| StyleFlags {
                bold: self.at_bold.current_value.unwrap_or(false),
                italic: self.at_italic.current_value.unwrap_or(false),
                strike: self.at_strike.current_value.unwrap_or(false),
            })
    }

    pub(crate) fn at_decoration_override(&self) -> Option<u8> {
        self.at_decoration
            .input_override
            .then_some(self.at_decoration.current_value)
            .flatten()
    }

    pub(crate) fn at_style_component_overrides(&self) -> StyleFlags {
        StyleFlags {
            bold: self.at_bold.input_override,
            italic: self.at_italic.input_override,
            strike: self.at_strike.input_override,
        }
    }

    fn effective_font_state(&self) -> &AttributeState<String> {
        if self.s_font_family.input_override {
            &self.s_font_family
        } else {
            &self.at_font_family
        }
    }

    fn clear_s_optional_format(&mut self) {
        self.s_font_family.reset();
        self.s_bold.reset();
        self.s_italic.reset();
        self.s_strike.reset();
        self.s_outline_size.reset();
        self.s_style_active = false;
    }

    fn sync_s_resets_from_at(&mut self) {
        self.s_font_family
            .set_reset_value(self.at_font_family.current_value.clone());
        self.s_bold.set_reset_value(self.at_bold.current_value);
        self.s_italic.set_reset_value(self.at_italic.current_value);
        self.s_strike.set_reset_value(self.at_strike.current_value);
    }

    fn reset_at_format(&mut self) {
        self.at_font_family.reset();
        self.at_decoration.reset();
        self.at_bold.reset();
        self.at_italic.reset();
        self.at_strike.reset();
        self.s_font_family.reset();
        self.s_bold.reset();
        self.s_italic.reset();
        self.s_strike.reset();
        self.s_outline_size.reset();
        self.s_style_active = false;
        self.sync_s_resets_from_at();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParsedSpanKind {
    Visible,
    Newline,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParsedSpan {
    pub(crate) range: Range<usize>,
    pub(crate) kind: ParsedSpanKind,
    pub(crate) format: TextFormatState,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParsedControlText {
    pub(crate) spans: Vec<ParsedSpan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ParseError;

#[derive(Clone, Debug, PartialEq)]
enum BasicFormatTag {
    Color {
        text: Option<String>,
        decoration: Option<String>,
        reset: bool,
    },
    Size(SizeTag),
    Font {
        font: Option<String>,
        style: Option<FontStyle>,
        reset: bool,
    },
    StyleDelta {
        add: bool,
        styles: StyleFlags,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct SizeTag {
    size: Option<SizeExpression>,
    size_field_is_empty: bool,
    font: Option<String>,
    style: Option<StyleFlags>,
    outline_size: Option<String>,
    reset: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct FontStyle {
    decoration: Option<u8>,
    styles: StyleFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SizeExpressionKind {
    Absolute,
    Add,
    Subtract,
    Multiply,
}

#[derive(Clone, Debug, PartialEq)]
struct SizeExpression {
    kind: SizeExpressionKind,
    value: f64,
    source: String,
}

pub(crate) fn parse_control_text(
    source: &str,
    defaults: ControlTextDefaults,
) -> Result<ParsedControlText, ParseError> {
    let mut state = TextFormatState::new(&defaults);
    let mut spans = Vec::new();
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut visible_start = 0;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'<' => {
                push_visible_span(&mut spans, visible_start..cursor, &state);
                let tag_end = source[cursor + 1..]
                    .find('>')
                    .map(|relative| cursor + 1 + relative)
                    .ok_or(ParseError)?;
                let tag = parse_tag(&source[cursor + 1..tag_end])?;
                apply_tag(&mut state, tag)?;
                cursor = tag_end + 1;
                visible_start = cursor;
            }
            b'\r' | b'\n' => {
                push_visible_span(&mut spans, visible_start..cursor, &state);
                let end = if bytes[cursor] == b'\r' && bytes.get(cursor + 1).copied() == Some(b'\n')
                {
                    cursor + 2
                } else {
                    cursor + 1
                };
                spans.push(ParsedSpan {
                    range: cursor..end,
                    kind: ParsedSpanKind::Newline,
                    format: state.clone(),
                });
                cursor = end;
                visible_start = cursor;
            }
            _ => cursor += 1,
        }
    }
    push_visible_span(&mut spans, visible_start..source.len(), &state);
    Ok(ParsedControlText { spans })
}

fn push_visible_span(spans: &mut Vec<ParsedSpan>, range: Range<usize>, state: &TextFormatState) {
    if !range.is_empty() {
        spans.push(ParsedSpan {
            range,
            kind: ParsedSpanKind::Visible,
            format: state.clone(),
        });
    }
}

fn parse_tag(body: &str) -> Result<BasicFormatTag, ParseError> {
    if let Some(payload) = body.strip_prefix('#') {
        return parse_color_tag(payload);
    }
    if let Some(payload) = body.strip_prefix('s') {
        return parse_size_tag(payload).map(BasicFormatTag::Size);
    }
    if let Some(payload) = body.strip_prefix('@') {
        return parse_font_tag(payload);
    }
    Err(ParseError)
}

fn parse_color_tag(payload: &str) -> Result<BasicFormatTag, ParseError> {
    if payload.is_empty() {
        return Ok(BasicFormatTag::Color {
            text: None,
            decoration: None,
            reset: true,
        });
    }
    let fields = split_fields(payload, 2)?;
    let text = parse_optional_color(fields[0])?;
    let decoration = fields
        .get(1)
        .map(|field| parse_optional_color(field))
        .transpose()?
        .flatten();
    Ok(BasicFormatTag::Color {
        text,
        decoration,
        reset: true,
    })
}

fn parse_size_tag(payload: &str) -> Result<SizeTag, ParseError> {
    if payload.is_empty() {
        return Ok(SizeTag {
            size: None,
            size_field_is_empty: false,
            font: None,
            style: None,
            outline_size: None,
            reset: true,
        });
    }
    let fields = split_fields(payload, 4)?;
    let size_field_is_empty = fields[0].is_empty();
    let size = (!size_field_is_empty)
        .then(|| parse_size_expression(fields[0]))
        .transpose()?;
    let font = fields
        .get(1)
        .map(|field| parse_optional_name(field))
        .transpose()?
        .flatten();
    let style = fields
        .get(2)
        .map(|field| parse_style_flags(field))
        .transpose()?;
    let outline_size = match fields.get(3).copied() {
        Some("") => return Err(ParseError),
        Some(field) => {
            let value = parse_decimal(field, true)?;
            if value < 0.0 {
                return Err(ParseError);
            }
            Some(field.to_owned())
        }
        None => None,
    };
    Ok(SizeTag {
        size,
        size_field_is_empty,
        font,
        style,
        outline_size,
        reset: false,
    })
}

fn parse_font_tag(payload: &str) -> Result<BasicFormatTag, ParseError> {
    if payload.is_empty() {
        return Ok(BasicFormatTag::Font {
            font: None,
            style: None,
            reset: true,
        });
    }
    if let Some(styles) = payload.strip_prefix('+') {
        return Ok(BasicFormatTag::StyleDelta {
            add: true,
            styles: parse_nonempty_style_flags(styles)?,
        });
    }
    if let Some(styles) = payload.strip_prefix('-') {
        return Ok(BasicFormatTag::StyleDelta {
            add: false,
            styles: parse_nonempty_style_flags(styles)?,
        });
    }
    let fields = split_fields(payload, 2)?;
    let font = parse_optional_name(fields[0])?;
    let style = fields
        .get(1)
        .map(|field| parse_font_style(field))
        .transpose()?;
    if font.is_none() && style.is_none() {
        return Err(ParseError);
    }
    Ok(BasicFormatTag::Font {
        font,
        style,
        reset: false,
    })
}

fn split_fields(value: &str, maximum: usize) -> Result<Vec<&str>, ParseError> {
    let fields = value.split(',').collect::<Vec<_>>();
    (fields.len() <= maximum)
        .then_some(fields)
        .ok_or(ParseError)
}

fn parse_optional_name(value: &str) -> Result<Option<String>, ParseError> {
    if value.is_empty() {
        return Ok(None);
    }
    if value
        .chars()
        .any(|character| matches!(character, '<' | '>' | ',') || character.is_control())
    {
        return Err(ParseError);
    }
    Ok(Some(value.to_owned()))
}

fn parse_optional_color(value: &str) -> Result<Option<String>, ParseError> {
    if value.is_empty() {
        return Ok(None);
    }
    let is_hex = value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
    let is_standard_alias = matches!(
        value,
        "white" | "red" | "yellow" | "green" | "aqua" | "blue" | "magenta" | "black"
    );
    (is_hex || is_standard_alias)
        .then(|| value.to_owned())
        .map(Some)
        .ok_or(ParseError)
}

fn parse_font_style(value: &str) -> Result<FontStyle, ParseError> {
    let (decoration, styles) = match value.as_bytes().first().copied() {
        Some(digit @ b'0'..=b'6') => (Some(digit - b'0'), parse_style_flags(&value[1..])?),
        Some(b'7'..=b'9') => return Err(ParseError),
        _ => (None, parse_style_flags(value)?),
    };
    Ok(FontStyle { decoration, styles })
}

fn parse_nonempty_style_flags(value: &str) -> Result<StyleFlags, ParseError> {
    if value.is_empty() {
        return Err(ParseError);
    }
    parse_style_flags(value)
}

fn parse_style_flags(value: &str) -> Result<StyleFlags, ParseError> {
    let mut styles = StyleFlags::default();
    for character in value.chars() {
        let target = match character {
            'B' => &mut styles.bold,
            'I' => &mut styles.italic,
            'S' => &mut styles.strike,
            _ => return Err(ParseError),
        };
        if *target {
            return Err(ParseError);
        }
        *target = true;
    }
    Ok(styles)
}

fn parse_size_expression(value: &str) -> Result<SizeExpression, ParseError> {
    let (kind, number) = match value.as_bytes().first().copied() {
        Some(b'+') => (SizeExpressionKind::Add, &value[1..]),
        Some(b'-') => (SizeExpressionKind::Subtract, &value[1..]),
        Some(b'*') => (SizeExpressionKind::Multiply, &value[1..]),
        _ => (SizeExpressionKind::Absolute, value),
    };
    if number.starts_with(['+', '-']) {
        return Err(ParseError);
    }
    let number = parse_decimal(number, true)?;
    if number <= 0.0
        && matches!(
            kind,
            SizeExpressionKind::Absolute | SizeExpressionKind::Multiply
        )
    {
        return Err(ParseError);
    }
    Ok(SizeExpression {
        kind,
        value: number,
        source: value.to_owned(),
    })
}

fn parse_decimal(value: &str, allow_zero: bool) -> Result<f64, ParseError> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return Err(ParseError);
    }
    let digits = if matches!(bytes[0], b'+' | b'-') {
        &bytes[1..]
    } else {
        bytes
    };
    if digits.is_empty() {
        return Err(ParseError);
    }
    let mut dot_seen = false;
    let mut digit_seen = false;
    for byte in digits {
        match byte {
            b'0'..=b'9' => digit_seen = true,
            b'.' if !dot_seen => dot_seen = true,
            _ => return Err(ParseError),
        }
    }
    if !digit_seen {
        return Err(ParseError);
    }
    let parsed = value.parse::<f64>().map_err(|_| ParseError)?;
    if !parsed.is_finite() || (!allow_zero && parsed == 0.0) {
        return Err(ParseError);
    }
    Ok(parsed)
}

fn apply_tag(state: &mut TextFormatState, tag: BasicFormatTag) -> Result<(), ParseError> {
    match tag {
        BasicFormatTag::Color {
            text,
            decoration,
            reset,
        } => {
            debug_assert!(reset);
            state.text_color.reset();
            state.decoration_color.reset();
            if let Some(text) = text {
                state.text_color.set(text);
            }
            if let Some(decoration) = decoration {
                state.decoration_color.set(decoration);
            }
        }
        BasicFormatTag::Size(tag) => apply_size_tag(state, tag)?,
        BasicFormatTag::Font { font, style, reset } => {
            if let Some(font) = font.filter(|_| !reset) {
                state.reset_at_format();
                state.at_font_family.set(font);
                if let Some(style) = style {
                    if let Some(decoration) = style.decoration {
                        state.at_decoration.set(decoration);
                    }
                    set_at_styles(state, style.styles);
                }
                state.sync_s_resets_from_at();
            } else {
                state.reset_at_format();
            }
        }
        BasicFormatTag::StyleDelta { add, styles } => {
            apply_style_delta(&mut state.at_bold, styles.bold, add);
            apply_style_delta(&mut state.at_italic, styles.italic, add);
            apply_style_delta(&mut state.at_strike, styles.strike, add);
            if state.s_style_active {
                apply_style_delta(&mut state.s_bold, styles.bold, add);
                apply_style_delta(&mut state.s_italic, styles.italic, add);
                apply_style_delta(&mut state.s_strike, styles.strike, add);
            }
            state.sync_s_resets_from_at();
        }
    }
    Ok(())
}

fn apply_size_tag(state: &mut TextFormatState, tag: SizeTag) -> Result<(), ParseError> {
    if tag.reset {
        state.font_size.reset();
        state.size_restore_expressions.clear();
        state.clear_s_optional_format();
        return Ok(());
    }

    let previous_size = state.font_size.current_value;
    state.clear_s_optional_format();
    if tag.size_field_is_empty {
        state.font_size.reset();
        state.size_restore_expressions.clear();
    } else if let Some(expression) = tag.size {
        let value = match expression.kind {
            SizeExpressionKind::Absolute => Some(expression.value),
            SizeExpressionKind::Add => previous_size.map(|value| value + expression.value),
            SizeExpressionKind::Subtract => previous_size.map(|value| value - expression.value),
            SizeExpressionKind::Multiply => previous_size.map(|value| value * expression.value),
        };
        if value.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(ParseError);
        }
        state.font_size.input_override = true;
        state.font_size.current_value = value;
        if expression.kind == SizeExpressionKind::Absolute {
            state.size_restore_expressions.clear();
        }
        state.size_restore_expressions.push(expression.source);
    }
    if let Some(font) = tag.font {
        state.s_font_family.set(font);
    }
    if let Some(styles) = tag.style {
        state.s_bold.set(styles.bold);
        state.s_italic.set(styles.italic);
        state.s_strike.set(styles.strike);
        state.s_style_active = true;
    }
    if let Some(outline_size) = tag.outline_size {
        state.s_outline_size.set(outline_size);
    }
    Ok(())
}

fn set_at_styles(state: &mut TextFormatState, styles: StyleFlags) {
    state.at_bold.set(styles.bold);
    state.at_italic.set(styles.italic);
    state.at_strike.set(styles.strike);
}

fn apply_style_delta(attribute: &mut AttributeState<bool>, selected: bool, value: bool) {
    if selected {
        attribute.set(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> ControlTextDefaults {
        ControlTextDefaults {
            font_size: Some(40.0),
            font_family: Some("Object Font".to_owned()),
        }
    }

    #[test]
    fn tokenizes_visible_text_tags_and_original_newlines() {
        let source = "国<#ff0000>LINE<#>\r\n<s120>123<s>\n";
        let parsed = parse_control_text(source, defaults()).unwrap();
        assert_eq!(
            parsed
                .spans
                .iter()
                .map(|span| (&source[span.range.clone()], span.kind))
                .collect::<Vec<_>>(),
            vec![
                ("国", ParsedSpanKind::Visible),
                ("LINE", ParsedSpanKind::Visible),
                ("\r\n", ParsedSpanKind::Newline),
                ("123", ParsedSpanKind::Visible),
                ("\n", ParsedSpanKind::Newline),
            ]
        );
    }

    #[test]
    fn tracks_only_fields_explicitly_owned_by_input_tags() {
        let source = "<s120>1<s,User Font>2<@At Font,6BI>3<#ff0000>4";
        let parsed = parse_control_text(source, defaults()).unwrap();
        let one = &parsed.spans[0].format;
        assert!(!one.profile_size_allowed());
        assert!(one.profile_font_allowed());
        let two = &parsed.spans[1].format;
        assert!(two.profile_size_allowed());
        assert!(!two.profile_font_allowed());
        assert_eq!(two.effective_font_family(), Some("User Font"));
        let three = &parsed.spans[2].format;
        assert_eq!(three.effective_font_family(), Some("At Font"));
        assert_eq!(three.at_decoration_override(), Some(6));
        assert_eq!(
            three.at_style_override(),
            Some(StyleFlags {
                bold: true,
                italic: true,
                strike: false,
            })
        );
        let four = &parsed.spans[3].format;
        assert!(four.text_color.input_override);
        assert!(!four.decoration_color.input_override);
    }

    #[test]
    fn color_tags_reset_omitted_fields_before_applying_present_fields() {
        let parsed =
            parse_control_text("<#red,green>A<#blue>B<#,yellow>C<#,>D", defaults()).unwrap();
        let states = parsed
            .spans
            .iter()
            .map(|span| &span.format)
            .collect::<Vec<_>>();

        assert_eq!(states[0].text_color.current_value.as_deref(), Some("red"));
        assert_eq!(
            states[0].decoration_color.current_value.as_deref(),
            Some("green")
        );
        assert_eq!(states[1].text_color.current_value.as_deref(), Some("blue"));
        assert!(!states[1].decoration_color.input_override);
        assert!(!states[2].text_color.input_override);
        assert_eq!(
            states[2].decoration_color.current_value.as_deref(),
            Some("yellow")
        );
        assert!(!states[3].text_color.input_override);
        assert!(!states[3].decoration_color.input_override);
    }

    #[test]
    fn size_reset_returns_to_object_defaults_and_reenables_profile_size() {
        let parsed = parse_control_text("<s120>A<s>B", defaults()).unwrap();
        assert_eq!(parsed.spans[0].format.effective_font_size(), Some(120.0));
        assert!(!parsed.spans[0].format.profile_size_allowed());
        assert_eq!(parsed.spans[1].format.effective_font_size(), Some(40.0));
        assert!(parsed.spans[1].format.profile_size_allowed());
    }

    #[test]
    fn empty_size_field_resets_size_but_applies_other_s_fields() {
        let parsed = parse_control_text("<s80><s,User Font,,8>A", defaults()).unwrap();
        let state = &parsed.spans[0].format;
        assert_eq!(state.effective_font_size(), Some(40.0));
        assert!(state.profile_size_allowed());
        assert_eq!(state.s_font_override(), Some("User Font"));
        assert_eq!(state.s_style_override(), Some(StyleFlags::default()));
        assert_eq!(state.s_outline_override(), Some("8"));
    }

    #[test]
    fn relative_sizes_follow_aviutl_order() {
        let parsed = parse_control_text("<s80><s+10><s*1.5>A", defaults()).unwrap();
        assert_eq!(parsed.spans[0].format.effective_font_size(), Some(135.0));
        assert_eq!(
            parsed.spans[0].format.size_restore_expressions(),
            &["80", "+10", "*1.5"]
        );
    }

    #[test]
    fn at_reset_clears_font_decoration_and_styles_but_keeps_size() {
        let parsed =
            parse_control_text("<s80,Size Font,B><@User Font,6IS>A<@>B", defaults()).unwrap();
        let before = &parsed.spans[0].format;
        assert_eq!(before.effective_font_size(), Some(80.0));
        assert_eq!(before.effective_font_family(), Some("User Font"));
        assert_eq!(before.at_decoration_override(), Some(6));
        let after = &parsed.spans[1].format;
        assert_eq!(after.effective_font_size(), Some(80.0));
        assert_eq!(after.effective_font_family(), Some("Object Font"));
        assert!(after.at_style_override().is_none());
    }

    #[test]
    fn style_deltas_update_active_s_style_and_its_reset_base() {
        let parsed = parse_control_text("<s80,,B><@+I>A<s>B", defaults()).unwrap();
        assert_eq!(
            parsed.spans[0].format.s_style_override(),
            Some(StyleFlags {
                bold: true,
                italic: true,
                strike: false,
            })
        );
        assert_eq!(
            parsed.spans[1].format.at_style_override(),
            Some(StyleFlags {
                bold: false,
                italic: true,
                strike: false,
            })
        );
    }

    #[test]
    fn consecutive_basic_tags_preserve_non_nested_attribute_scope() {
        let parsed = parse_control_text("<@+B><#red>A<@+I>B<@-B><#>C<@-I>D", defaults()).unwrap();
        let state = |index: usize| &parsed.spans[index].format;

        assert_eq!(state(0).at_bold.current_value, Some(true));
        assert!(!state(0).at_italic.input_override);
        assert_eq!(state(0).text_color.current_value.as_deref(), Some("red"));
        assert_eq!(state(1).at_bold.current_value, Some(true));
        assert_eq!(state(1).at_italic.current_value, Some(true));
        assert_eq!(state(2).at_bold.current_value, Some(false));
        assert_eq!(state(2).at_italic.current_value, Some(true));
        assert!(!state(2).text_color.input_override);
        assert_eq!(state(3).at_italic.current_value, Some(false));
    }

    #[test]
    fn rejects_unsupported_unknown_and_malformed_tags_before_output() {
        for source in [
            "<?obj.rz=1?>国",
            "</>漢字<!>かんじ</>",
            "<// comment //>国",
            "<future-control>国",
            "閉じていない<",
            "<snope>国",
            "<@+X>国",
            "<s80,,,>国",
            "<#red,blue,green>国",
            "<#custom-palette-alias>国",
        ] {
            assert_eq!(parse_control_text(source, defaults()), Err(ParseError));
        }
    }
}
