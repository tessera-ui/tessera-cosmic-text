use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

use core_foundation::{
    array::{CFArray, CFArrayRef},
    base::TCFType,
    data::{CFData, CFDataRef},
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use core_graphics::{
    base::kCGImageAlphaPremultipliedLast,
    color_space::CGColorSpace,
    context::CGContext,
    geometry::{CGPoint, CGRect, CGSize},
};
use core_text::{
    font::{self, CTFont},
    font_descriptor::{self, CTFontDescriptor},
};

use crate::{CacheKey, Font, FontSystem, SwashContent, SwashImage};

extern "C" {
    fn CTFontManagerCreateFontDescriptorsFromData(data: CFDataRef) -> CFArrayRef;
}

struct NativeFont {
    source: Weak<Font>,
    points: u32,
    font: Option<CTFont>,
}

#[derive(Default)]
pub(super) struct Rasterizer {
    fonts: VecDeque<NativeFont>,
}

impl Rasterizer {
    fn font(&mut self, fonts: &mut FontSystem, key: CacheKey, points: f32) -> Option<CTFont> {
        let source =
            fonts.get_font_with_optical_size(key.font_id, key.font_weight, key.optical_size)?;
        self.fonts.retain(|entry| entry.source.strong_count() != 0);
        let identity = Arc::downgrade(&source);
        if let Some(index) = self
            .fonts
            .iter()
            .position(|entry| entry.source.ptr_eq(&identity) && entry.points == points.to_bits())
        {
            let entry = self.fonts.remove(index)?;
            let font = entry.font.clone();
            self.fonts.push_back(entry);
            return font;
        }

        let data = CFData::from_buffer(source.data());
        // The collection index must agree with shaping, including embedded TTC faces.
        let descriptors = unsafe {
            let raw = CTFontManagerCreateFontDescriptorsFromData(data.as_concrete_TypeRef());
            if raw.is_null() {
                return None;
            }
            CFArray::<CTFontDescriptor>::wrap_under_create_rule(raw)
        };
        let descriptor = descriptors.get(fonts.db().face(key.font_id)?.index as isize)?;
        let mut values = vec![(
            CFNumber::from(i64::from(u32::from_be_bytes(*b"wght"))),
            CFNumber::from(f64::from(key.font_weight.0)),
        )];
        if let Some(size) = key.optical_size {
            values.push((
                CFNumber::from(i64::from(u32::from_be_bytes(*b"opsz"))),
                CFNumber::from(f64::from(size.points())),
            ));
        }
        let variation_key =
            unsafe { CFString::wrap_under_get_rule(font_descriptor::kCTFontVariationAttribute) };
        let values = CFDictionary::from_CFType_pairs(&values);
        let attributes = CFDictionary::from_CFType_pairs(&[(variation_key, values.as_CFType())]);
        let descriptor = descriptor
            .create_copy_with_attributes(attributes.to_untyped())
            .ok()?;
        let font = font::new_from_descriptor(&descriptor, f64::from(points));
        let font = (![*b"sbix", *b"COLR", *b"CBDT", *b"SVG "]
            .iter()
            .any(|tag| font.get_font_table(u32::from_be_bytes(*tag)).is_some()))
        .then_some(font);
        if self.fonts.len() == 32 {
            self.fonts.pop_front();
        }
        self.fonts.push_back(NativeFont {
            source: identity,
            points: points.to_bits(),
            font: font.clone(),
        });
        font
    }

    pub(super) fn rasterize(
        &mut self,
        fonts: &mut FontSystem,
        key: CacheKey,
    ) -> Option<SwashImage> {
        let pixels = f32::from_bits(key.font_size_bits);
        if !pixels.is_finite() || pixels <= 0.0 {
            return None;
        }
        let points = key.point_size.map_or(pixels, |size| size.points());
        let font = self.font(fonts, key, points)?;
        let scale = f64::from(pixels) / f64::from(points);
        let bounds = font.get_bounding_rects_for_glyphs(
            font_descriptor::kCTFontOrientationHorizontal,
            &[key.glyph_id],
        );
        let x = f64::from(key.x_bin.as_float());
        let y = -f64::from(key.y_bin.as_float());
        let left = ((bounds.origin.x * scale + x).floor() as i32).checked_sub(1)?;
        let bottom = ((bounds.origin.y * scale + y).floor() as i32).checked_sub(1)?;
        let right =
            (((bounds.origin.x + bounds.size.width) * scale + x).ceil() as i32).checked_add(1)?;
        let top =
            (((bounds.origin.y + bounds.size.height) * scale + y).ceil() as i32).checked_add(1)?;
        let width = u32::try_from(right.checked_sub(left)?).ok()?;
        let height = u32::try_from(top.checked_sub(bottom)?).ok()?;
        let bytes = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if bytes == 0 || bytes > 16 * 1024 * 1024 {
            return None;
        }
        let space = CGColorSpace::create_device_rgb();
        let mut context = CGContext::create_bitmap_context(
            None,
            width as usize,
            height as usize,
            8,
            width as usize * 4,
            &space,
            kCGImageAlphaPremultipliedLast,
        );
        let light = key.flags.contains(crate::CacheKeyFlags::NATIVE_LIGHT_INK);
        let background = if light { 0.0 } else { 1.0 };
        let foreground = 1.0 - background;
        context.set_rgb_fill_color(background, background, background, 1.0);
        context.fill_rect(CGRect::new(
            &CGPoint::new(0.0, 0.0),
            &CGSize::new(width as f64, height as f64),
        ));
        context.set_rgb_fill_color(foreground, foreground, foreground, 1.0);
        context.set_allows_antialiasing(true);
        context.set_should_antialias(true);
        context.set_allows_font_smoothing(true);
        context.set_should_smooth_fonts(true);
        context.set_allows_font_subpixel_positioning(true);
        context.set_should_subpixel_position_fonts(true);
        context.set_allows_font_subpixel_quantization(true);
        context.set_should_subpixel_quantize_fonts(true);
        context.scale(scale, scale);
        font.draw_glyphs(
            &[key.glyph_id],
            &[CGPoint::new(
                (x - left as f64) / scale,
                (y - bottom as f64) / scale,
            )],
            context.clone(),
        );
        let data = context
            .data()
            .chunks_exact(4)
            .map(|pixel| if light { pixel[0] } else { 255 - pixel[0] })
            .collect();
        Some(SwashImage {
            source: swash::scale::Source::Outline,
            content: SwashContent::Mask,
            placement: swash::zeno::Placement {
                left,
                top,
                width,
                height,
            },
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> FontSystem {
        let mut database = fontdb::Database::new();
        database.load_font_data(include_bytes!("../fonts/InterVariable.ttf").to_vec());
        FontSystem::new_with_locale_and_db("en-US".into(), database)
    }

    fn key(fonts: &mut FontSystem) -> CacheKey {
        let id = fonts.db().faces().next().unwrap().id;
        let glyph = fonts
            .get_font(id, fontdb::Weight::NORMAL)
            .unwrap()
            .as_swash()
            .charmap()
            .map('H');
        let mut key = CacheKey::new(
            id,
            glyph,
            26.0,
            (0.0, 0.0),
            fontdb::Weight::NORMAL,
            crate::CacheKeyFlags::NATIVE_RASTER,
        )
        .0;
        key.point_size = Some(crate::FontSize::new(13.0));
        key.optical_size = Some(crate::FontSize::new(17.0));
        key
    }

    #[test]
    fn native_masks_use_core_text_and_keep_fractional_origins() {
        let mut fonts = fonts();
        let key = key(&mut fonts);
        let mut cache = crate::SwashCache::new();
        let native = cache.get_image_uncached(&mut fonts, key).unwrap();
        assert_eq!(native.content, SwashContent::Mask);
        assert!(native.data.iter().any(|value| *value != 0));
        let mut shifted = key;
        shifted.x_bin = crate::SubpixelBin::Three;
        let shifted = cache.get_image_uncached(&mut fonts, shifted).unwrap();
        assert_ne!(native.data, shifted.data);
        let mut light = key;
        light.flags |= crate::CacheKeyFlags::NATIVE_LIGHT_INK;
        let light = cache.get_image_uncached(&mut fonts, light).unwrap();
        assert!(light.data.iter().any(|value| *value != 0));
        assert_ne!(native.data, light.data);
        let mut portable = key;
        portable.flags = crate::CacheKeyFlags::empty();
        let portable = cache.get_image_uncached(&mut fonts, portable).unwrap();
        assert_ne!(native.data, portable.data);
        assert_eq!(
            cache.get_image(&mut fonts, key).as_ref().unwrap().data,
            native.data
        );
    }

    #[test]
    fn native_font_instances_are_reused_bounded_and_do_not_retain_the_font_system() {
        let mut fonts = fonts();
        let mut key = key(&mut fonts);
        let mut rasterizer = Rasterizer::default();
        let first = rasterizer.font(&mut fonts, key, 13.0).unwrap();
        let repeated = rasterizer.font(&mut fonts, key, 13.0).unwrap();
        assert_eq!(first.as_concrete_TypeRef(), repeated.as_concrete_TypeRef());
        for weight in 400..440 {
            key.font_weight = fontdb::Weight(weight);
            rasterizer.font(&mut fonts, key, 13.0).unwrap();
        }
        assert_eq!(rasterizer.fonts.len(), 32);
        drop(fonts);
        let mut fonts = self::fonts();
        let key = self::key(&mut fonts);
        rasterizer.font(&mut fonts, key, 13.0).unwrap();
        assert_eq!(rasterizer.fonts.len(), 1);
    }
}
