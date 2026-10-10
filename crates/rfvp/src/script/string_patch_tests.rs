use super::*;
use crate::script::{parser::Nls, string_patch::StringReplacements};

fn parser_with_string(nls: Nls) -> Parser {
    let code = [0x0e, 3, 0x82, 0xa0, 0, 0x42]; // Shift-JIS あ, then next opcode
    let mut bytes = (4u32 + code.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(&code);
    bytes.extend_from_slice(&[4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, b'T', 0, 0, 0, 0, 0]);
    Parser::from_bytes(bytes, nls).unwrap()
}

#[test]
fn replacements_preserve_original_address_cursor_nls_and_metadata() {
    let strings = StringReplacements::new(4..10, [(6, "更长的中文翻译".into())]).unwrap();
    let mut parser = parser_with_string(Nls::ShiftJIS)
        .with_string_replacements(strings)
        .unwrap();
    assert_eq!(parser.read_cstring(6, 3).unwrap(), "あ");
    assert_eq!(parser.nls, Nls::ShiftJIS);
    assert_eq!(parser.get_title(), "T");
    let mut ctx = Context::new(4, 0);
    ctx.push_string(&mut parser).unwrap();
    assert_eq!(ctx.cursor, 9);
    assert_eq!(parser.read_u8(ctx.cursor).unwrap(), 0x42);
    match ctx.pop().unwrap() {
        Variant::ConstString(text, address) => {
            assert_eq!(text, "更长的中文翻译");
            assert_eq!(address, 6);
        }
        other => panic!("expected ConstString, got {other:?}"),
    }
}

#[test]
fn disabled_and_missing_entries_use_original_text() {
    for replacements in [
        None,
        Some(StringReplacements::new(4..10, [(7, "unused".into())]).unwrap()),
    ] {
        let mut parser = parser_with_string(Nls::ShiftJIS);
        if let Some(strings) = replacements {
            parser = parser.with_string_replacements(strings).unwrap();
        }
        let mut ctx = Context::new(4, 0);
        ctx.push_string(&mut parser).unwrap();
        assert_eq!(ctx.cursor, 9);
        match ctx.pop().unwrap() {
            Variant::ConstString(text, address) => {
                assert_eq!(text, "あ");
                assert_eq!(address, 6);
            }
            other => panic!("expected ConstString, got {other:?}"),
        }
    }
}

#[test]
fn replacement_does_not_mask_original_bounds_failure() {
    let strings = StringReplacements::new(4..10, [(6, "replacement".into())]).unwrap();
    let mut parser = parser_with_string(Nls::ShiftJIS)
        .with_string_replacements(strings)
        .unwrap();
    alloc::sync::Arc::make_mut(&mut parser.buffer)[5] = 255;
    let mut ctx = Context::new(4, 0);
    assert!(ctx.push_string(&mut parser).is_err());
    assert_eq!(ctx.cursor, 6);
}

#[test]
fn empty_replacement_is_not_fallback_and_attachment_checks_script_range() {
    let strings = StringReplacements::new(4..10, [(6, String::new())]).unwrap();
    let mut parser = parser_with_string(Nls::ShiftJIS)
        .with_string_replacements(strings)
        .unwrap();
    let mut ctx = Context::new(4, 0);
    ctx.push_string(&mut parser).unwrap();
    assert!(matches!(ctx.pop().unwrap(), Variant::ConstString(text, 6) if text.is_empty()));
    let outside = StringReplacements::new(0..100, [(50, "outside".into())]).unwrap();
    assert!(parser.with_string_replacements(outside).is_err());
}
