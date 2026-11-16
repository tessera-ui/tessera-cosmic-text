use cosmic_text::{
    fontdb, Attrs, Buffer, Family, FontSize, FontSystem, Metrics, Shaping, SwashCache,
};

fn fonts() -> FontSystem {
    let mut db = fontdb::Database::new();
    db.load_font_data(include_bytes!("../fonts/InterVariable.ttf").to_vec());
    FontSystem::new_with_locale_and_db("en-US".into(), db)
}

fn shape(fonts: &mut FontSystem, optical_size: f32, size: f32) -> Buffer {
    let mut buffer = Buffer::new(fonts, Metrics::new(size, size * 1.5));
    buffer.set_size(Some(2000.0), Some(200.0));
    buffer.set_text(
        "Hamburgefontsiv 0123",
        &Attrs::new()
            .family(Family::Name("Inter Variable"))
            .optical_size(optical_size),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(fonts, false);
    buffer
}

#[test]
fn optical_size_changes_shaping_and_cached_rasterization_independently_of_scale() {
    let mut fonts = fonts();
    let small = shape(&mut fonts, 14.0, 32.0);
    let large = shape(&mut fonts, 32.0, 32.0);
    let small_run = small.layout_runs().next().unwrap();
    let large_run = large.layout_runs().next().unwrap();
    assert!((small_run.line_w - large_run.line_w).abs() > 1.0);

    let first = small_run.glyphs[0].physical((0.0, 0.0), 1.0).cache_key;
    let second = large_run.glyphs[0].physical((0.0, 0.0), 1.0).cache_key;
    assert_ne!(first, second);
    let doubled = small_run.glyphs[0].physical((0.0, 0.0), 2.0).cache_key;
    assert_eq!(doubled.optical_size, Some(FontSize::new(14.0)));
    assert_eq!(f32::from_bits(doubled.font_size_bits), 64.0);

    let mut cache = SwashCache::new();
    let original = cache.get_image(&mut fonts, first).clone().unwrap();
    let changed = cache.get_image(&mut fonts, second).clone().unwrap();
    assert_ne!(original.data, changed.data);
    assert_eq!(
        cache.get_image(&mut fonts, first).as_ref().unwrap().data,
        original.data
    );

    let restored = shape(&mut fonts, 14.0, 32.0);
    assert_eq!(
        restored.layout_runs().next().unwrap().line_w,
        small_run.line_w
    );
    let scaled = shape(&mut fonts, 14.0, 64.0);
    assert!((scaled.layout_runs().next().unwrap().line_w - small_run.line_w * 2.0).abs() < 0.01);
}

#[test]
fn rich_text_keeps_distinct_optical_sizes_in_adjacent_spans() {
    let mut fonts = fonts();
    let small = Attrs::new()
        .family(Family::Name("Inter Variable"))
        .optical_size(14.0);
    let large = Attrs::new()
        .family(Family::Name("Inter Variable"))
        .optical_size(32.0);
    let mut buffer = Buffer::new(&mut fonts, Metrics::new(32.0, 48.0));
    buffer.set_rich_text(
        [("Hamburge", small.clone()), ("Hamburge", large)],
        &small,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts, false);
    let glyphs = buffer.layout_runs().next().unwrap().glyphs;
    assert!(glyphs[..8]
        .iter()
        .all(|glyph| glyph.optical_size == Some(FontSize::new(14.0))));
    assert!(glyphs[8..]
        .iter()
        .all(|glyph| glyph.optical_size == Some(FontSize::new(32.0))));
    let first: f32 = glyphs[..8].iter().map(|glyph| glyph.w).sum();
    let second: f32 = glyphs[8..].iter().map(|glyph| glyph.w).sum();
    assert!((first - second).abs() > 1.0);
}

#[test]
fn basic_shaping_applies_optical_size_to_advances() {
    let mut fonts = fonts();
    let mut widths = Vec::new();
    for optical_size in [14.0, 32.0, 14.0] {
        let mut buffer = Buffer::new(&mut fonts, Metrics::new(32.0, 48.0));
        buffer.set_text(
            "Hamburgefontsiv",
            &Attrs::new().family(Family::Name("Inter Variable")).optical_size(optical_size),
            Shaping::Basic,
            None,
        );
        buffer.shape_until_scroll(&mut fonts, false);
        widths.push(buffer.layout_runs().next().unwrap().line_w);
    }
    assert!((widths[0] - widths[1]).abs() > 1.0);
    assert_eq!(widths[0], widths[2]);
}
