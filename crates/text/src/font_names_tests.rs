//! Reproduce numeric subfamilies without redistributing proprietary fonts.
use super::*;
use photocraft_doc::text::CharStyle;

fn numeric_font(source: &[u8], style: &str, ps: &str) -> Vec<u8> {
    let mut bytes = source.to_vec();
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    let mut name_record = 0;
    for i in 0..count {
        let r = 12 + i * 16;
        let offset = u32::from_be_bytes(bytes[r + 8..r + 12].try_into().unwrap()) as usize;
        match &bytes[r..r + 4] {
            b"name" => name_record = r,
            b"OS/2" => bytes[offset + 4..offset + 6].copy_from_slice(&400u16.to_be_bytes()),
            _ => {}
        }
    }
    let legacy = format!("Numeric Test {style}");
    let names = [(1u16, legacy.as_str()), (2, "Regular"), (6, ps), (16, "Numeric Test"), (17, style)];
    let mut table = Vec::new();
    let mut strings = Vec::new();
    for value in [0u16, names.len() as u16, (6 + names.len() * 12) as u16] {
        table.extend(value.to_be_bytes());
    }
    for (id, value) in names {
        let encoded: Vec<_> = value.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for n in [3, 1, 0x409, id, encoded.len() as u16, strings.len() as u16] {
            table.extend(n.to_be_bytes());
        }
        strings.extend(encoded);
    }
    table.extend(strings);
    let offset = bytes.len() as u32;
    bytes[name_record + 8..name_record + 12].copy_from_slice(&offset.to_be_bytes());
    bytes[name_record + 12..name_record + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
    bytes.extend(table);
    bytes
}

#[test]
fn numeric_subfamilies_with_identical_weights_select_distinct_faces() {
    let mut db = FontDb::new();
    db.register_font_data(numeric_font(&INTER_REGULAR, "20", "UnrelatedPS20"));
    db.register_font_data(numeric_font(&INTER_SEMIBOLD, "30", "UnrelatedPS30"));
    let faces = db.faces("Numeric Test");
    assert_eq!(faces.iter().map(|f| f.style.as_str()).collect::<Vec<_>>(), ["20", "30"]);
    assert!(faces.iter().all(|f| f.weight == 400.0));
    assert_eq!(db.resolve_postscript("UnrelatedPS30").family, "Numeric Test");
    assert!(db.resolve_postscript("UnrelatedPS30").exact);
    let mut a = CharStyle { font_family: "Numeric Test".into(), font_style: "20".into(), ..Default::default() };
    let mut b = CharStyle { font_style: "30".into(), ..a.clone() };
    db.select_named_face(&mut a);
    db.select_named_face(&mut b);
    assert_ne!(a.font_family, b.font_family);
    assert_eq!(db.faces(&a.font_family)[0].postscript_name.as_deref(), Some("UnrelatedPS20"));
    assert_eq!(db.faces(&b.font_family)[0].postscript_name.as_deref(), Some("UnrelatedPS30"));
    assert!(!db.families().iter().any(|f| f.starts_with(".PhotoCraft-face-")));
    let alias = b.font_family.clone();
    b.font_family = "Numeric Test".into();
    db.select_named_face(&mut b);
    assert_eq!(b.font_family, alias);
}

#[test]
fn numeric_styles_shape_different_outlines_and_keep_psd_identity() {
    use photocraft_doc::{TextLayer, text::TextRun};
    let mut engine = crate::TextEngine::new();
    engine.fonts.register_font_data(numeric_font(&INTER_REGULAR, "20", "UnrelatedPS20"));
    engine.fonts.register_font_data(numeric_font(&INTER_SEMIBOLD, "30", "UnrelatedPS30"));
    let layer = |style: &str| TextLayer {
        text: "Hamburgefonstiv".into(),
        runs: vec![TextRun {
            len: 14,
            style: CharStyle {
                font_family: "Numeric Test".into(),
                font_style: style.into(),
                size_pt: 32.0,
                postscript_name: Some(format!("UnrelatedPS{style}")),
                ..Default::default()
            },
        }],
        ..Default::default()
    };
    let a = engine.layout(&layer("20"), 72.0);
    let b = engine.layout(&layer("30"), 72.0);
    assert_ne!(a.lines[0].x1, b.lines[0].x1);
    let original = layer("30");
    let bytes = crate::psd::build_tysh(&original, 72.0, None);
    let reopened = crate::psd::text_layer_from_tysh(&bytes, 72.0).unwrap();
    assert_eq!(reopened.char_runs()[0].style.postscript_name.as_deref(), Some("UnrelatedPS30"));
    let c = engine.layout(&reopened, 72.0);
    assert!((b.lines[0].x1 - c.lines[0].x1).abs() < 0.01);
}

#[test]
fn explicit_weight_is_not_overridden_by_a_stale_style_name() {
    let mut db = FontDb::new();
    let mut style = CharStyle { font_family: "Inter".into(), font_style: "Regular".into(), weight: 600, ..Default::default() };
    db.select_named_face(&mut style);
    assert_eq!(style.font_family, "Inter");
    assert_eq!(style.weight, 600);
    assert!(db.named_face("Inter", "Medium").is_some());
    assert!(db.named_face("missing", "20").is_none());
    assert!(isolated_face(Blob::new(Arc::new(vec![0; 16])), 0).is_none());
}

#[test]
fn collection_isolation_preserves_requested_face() {
    // Two faces with identical attributes: the second must not fall back to the first.
    let sources = [numeric_font(&INTER_REGULAR, "20", "Collection20"), numeric_font(&INTER_SEMIBOLD, "30", "Collection30")];
    let mut bytes = b"ttcf\0\x01\0\0\0\0\0\x02".to_vec();
    bytes.extend(20u32.to_be_bytes());
    bytes.extend((20 + sources[0].len() as u32).to_be_bytes());
    for source in sources {
        let start = bytes.len();
        bytes.extend(source);
        let count = u16::from_be_bytes(bytes[start + 4..start + 6].try_into().unwrap()) as usize;
        for i in 0..count {
            let r = start + 12 + i * 16 + 8;
            let offset = u32::from_be_bytes(bytes[r..r + 4].try_into().unwrap()) + start as u32;
            bytes[r..r + 4].copy_from_slice(&offset.to_be_bytes());
        }
    }
    let blob = Blob::new(Arc::new(bytes));
    assert!(isolated_face(blob.clone(), 2).is_none());
    let mut db = FontDb::new();
    db.register_blob(blob.clone());
    let mut style = CharStyle { font_family: "Numeric Test".into(), font_style: "30".into(), ..Default::default() };
    db.select_named_face(&mut style);
    assert_eq!(db.faces(&style.font_family)[0].postscript_name.as_deref(), Some("Collection30"));
    let isolated = isolated_face(blob, 1).unwrap();
    let font = skrifa::FontRef::from_index(isolated.as_ref(), 0).unwrap();
    assert_eq!(font.localized_strings(StringId::POSTSCRIPT_NAME).english_or_first().unwrap().to_string(), "Collection30");
}

#[test]
#[ignore = "requires locally installed Yoon 500 fonts; no font files are distributed"]
fn installed_yoon_numeric_faces() {
    let mut db = FontDb::with_system_fonts();
    for family in ["Yoon YoonGothic500", "Yoon YoonMyungjo500"] {
        let faces = db.faces(family);
        for name in ["20", "30", "40", "50"] {
            let face = faces.iter().find(|f| f.style == name).expect("installed numeric face");
            let mut style =
                CharStyle { font_family: family.into(), font_style: name.into(), postscript_name: face.postscript_name.clone(), ..Default::default() };
            db.select_named_face(&mut style);
            assert_eq!(db.faces(&style.font_family)[0].postscript_name, face.postscript_name);
        }
    }
}

/// A copy of `source` whose name table spells the family in English and in the language whose
/// name-record language ID is `lang` (0x411 Japanese, 0x412 Korean).
fn localized_font(source: &[u8], english: &str, lang: u16, native: &str) -> Vec<u8> {
    let mut bytes = source.to_vec();
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    let name_record = (0..count).map(|i| 12 + i * 16).find(|r| &bytes[*r..r + 4] == b"name").unwrap();
    let ps = english.replace(' ', "");
    let names = [(0x409u16, 1u16, english), (0x409, 2, "Regular"), (0x409, 6, ps.as_str()), (0x409, 16, english), (lang, 1, native), (lang, 16, native)];
    let mut table = Vec::new();
    let mut strings = Vec::new();
    for value in [0u16, names.len() as u16, (6 + names.len() * 12) as u16] {
        table.extend(value.to_be_bytes());
    }
    for (lang, id, value) in names {
        let encoded: Vec<_> = value.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for n in [3, 1, lang, id, encoded.len() as u16, strings.len() as u16] {
            table.extend(n.to_be_bytes());
        }
        strings.extend(encoded);
    }
    table.extend(strings);
    let offset = bytes.len() as u32;
    bytes[name_record + 8..name_record + 12].copy_from_slice(&offset.to_be_bytes());
    bytes[name_record + 12..name_record + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
    bytes.extend(table);
    bytes
}

/// Show Font Names in English off: a font that names its family in Japanese is listed by that
/// name, keyed by the English family the menus and documents keep; English-only fonts are not.
#[test]
fn localized_family_names_map_english_families_to_native_names() {
    let mut db = FontDb::new();
    assert_eq!(db.register_font_data(localized_font(&INTER_REGULAR, "Gothic Test", 0x411, "ゴシック テスト")), ["Gothic Test"]);
    assert!(db.families().iter().any(|f| f == "Gothic Test"));
    let local = db.localized_family_names();
    assert_eq!(local.get("Gothic Test").map(String::as_str), Some("ゴシック テスト"));
    assert!(!local.contains_key(DEFAULT_FAMILY));
    assert!(local.values().all(|v| !v.is_empty()));
}

/// A font named in its own language ("맑은 고딕" for "Malgun Gothic", as Korean apps and users
/// write it) is found as its family: fontique only registers the English family name.
#[test]
fn a_family_is_found_by_its_native_name() {
    let mut db = FontDb::new();
    // A miss before the font arrives must not hide it afterwards.
    assert!(!db.has_family("고딕 테스트 Neo"));
    assert_eq!(db.register_font_data(localized_font(&INTER_SEMIBOLD, "Gothic Test Neo", 0x412, "고딕 테스트 Neo")), ["Gothic Test Neo"]);
    for name in ["고딕 테스트 Neo", "고딕 테스트 NEO"] {
        assert!(db.has_family(name), "{name}");
        assert_eq!(db.family_of_native(name).as_deref(), Some("Gothic Test Neo"), "{name}");
        assert_eq!(db.faces(name).first().map(|f| f.family.as_str()), Some("Gothic Test Neo"));
    }
    assert_eq!(db.charmap("고딕 테스트 Neo", 600, false), db.charmap("Gothic Test Neo", 600, false));
    // The English name is unchanged, and the native one is not listed as a family of its own.
    assert!(db.has_family("Gothic Test Neo"));
    assert_eq!(db.family_of_native("Gothic Test Neo"), None);
    assert!(!db.families().iter().any(|f| f == "고딕 테스트 Neo"));
    // Unknown names stay missing.
    for name in ["없는 글꼴", "고딕 테스트", ""] {
        assert!(!db.has_family(name), "{name}");
        assert_eq!(db.family_of_native(name), None, "{name}");
    }
    // A family that has the name itself wins over another font's native name.
    db.register_font_data(localized_font(&JETBRAINS_MONO_REGULAR, "고딕 테스트 Neo", 0x412, "모노 테스트"));
    assert_eq!(db.family_of_native("고딕 테스트 Neo"), None);
    assert_eq!(db.faces("고딕 테스트 Neo").first().map(|f| f.family.as_str()), Some("고딕 테스트 Neo"));
    assert_eq!(db.family_of_native("모노 테스트").as_deref(), Some("고딕 테스트 Neo"));
}

/// A run in a family named in its own language is laid out in that font, not the fallback.
#[test]
fn a_native_family_name_lays_out_in_that_font() {
    use photocraft_doc::{TextLayer, text::TextRun};
    let mut engine = crate::TextEngine::new();
    engine.fonts.register_font_data(localized_font(&JETBRAINS_MONO_REGULAR, "Mono Test", 0x412, "모노 테스트"));
    let layer = |family: &str| TextLayer {
        text: "iiii".into(),
        runs: vec![TextRun { len: 4, style: CharStyle { font_family: family.into(), size_pt: 32.0, ..Default::default() } }],
        ..Default::default()
    };
    let native = engine.layout(&layer("모노 테스트"), 72.0);
    let english = engine.layout(&layer("Mono Test"), 72.0);
    let fallback = engine.layout(&layer(DEFAULT_FAMILY), 72.0);
    assert!((native.lines[0].x1 - english.lines[0].x1).abs() < 0.01, "{} vs {}", native.lines[0].x1, english.lines[0].x1);
    assert!((native.lines[0].x1 - fallback.lines[0].x1).abs() > 1.0, "monospace i is wider than Inter's");
    assert_eq!(native.styles[0].font_family, "Mono Test");
}
