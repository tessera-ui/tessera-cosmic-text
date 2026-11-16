// SPDX-License-Identifier: MIT OR Apache-2.0

bitflags::bitflags! {
    /// Flags that change rendering
    #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
    #[repr(transparent)]
    pub struct CacheKeyFlags: u32 {
        /// Skew by 14 degrees to synthesize italic
        const FAKE_ITALIC = 1;
        /// Disable hinting
        const DISABLE_HINTING = 2;
        /// Render as a pixel font
        const PIXEL_FONT = 4;
        /// Use the platform text rasterizer where supported.
        const NATIVE_RASTER = 8;
        /// Rasterize native coverage for light text on a dark background.
        const NATIVE_LIGHT_INK = 16;
    }
}

/// Key for building a glyph cache
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CacheKey {
    /// Font ID
    pub font_id: fontdb::ID,
    /// Glyph ID
    pub glyph_id: u16,
    /// `f32` bits of font size
    pub font_size_bits: u32,
    /// Binning of fractional X offset
    pub x_bin: SubpixelBin,
    /// Binning of fractional Y offset
    pub y_bin: SubpixelBin,
    /// Font weight
    pub font_weight: fontdb::Weight,
    /// Optical size, independent of raster scale.
    pub optical_size: Option<crate::FontSize>,
    /// Point size used by the platform rasterizer, independent of raster scale.
    pub point_size: Option<crate::FontSize>,
    /// [`CacheKeyFlags`]
    pub flags: CacheKeyFlags,
}

impl CacheKey {
    pub fn new(
        font_id: fontdb::ID,
        glyph_id: u16,
        font_size: f32,
        pos: (f32, f32),
        weight: fontdb::Weight,
        flags: CacheKeyFlags,
    ) -> (Self, i32, i32) {
        let bin = if flags.contains(CacheKeyFlags::NATIVE_RASTER) {
            SubpixelBin::new_precise
        } else {
            SubpixelBin::new
        };
        let (x, x_bin) = bin(pos.0);
        let (y, y_bin) = bin(pos.1);
        (
            Self {
                font_id,
                glyph_id,
                font_size_bits: font_size.to_bits(),
                x_bin,
                y_bin,
                flags,
                font_weight: weight,
                optical_size: None,
                point_size: None,
            },
            x,
            y,
        )
    }
}

/// Binning of subpixel position for cache optimization
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SubpixelBin {
    Zero,
    One,
    Two,
    Three,
    /// A fractional offset in sixty-fourths of a pixel.
    Fine(u8),
}

impl SubpixelBin {
    fn new_precise(pos: f32) -> (i32, Self) {
        let floor = crate::math::floorf(pos);
        let fraction = crate::math::roundf((pos - floor) * 64.0) as u8;
        match fraction {
            0 => (floor as i32, Self::Zero),
            16 => (floor as i32, Self::One),
            32 => (floor as i32, Self::Two),
            48 => (floor as i32, Self::Three),
            64 => (floor as i32 + 1, Self::Zero),
            value => (floor as i32, Self::Fine(value)),
        }
    }

    pub fn new(pos: f32) -> (i32, Self) {
        let trunc = pos as i32;
        let fract = pos - trunc as f32;

        if pos.is_sign_negative() {
            if fract > -0.125 {
                (trunc, Self::Zero)
            } else if fract > -0.375 {
                (trunc - 1, Self::Three)
            } else if fract > -0.625 {
                (trunc - 1, Self::Two)
            } else if fract > -0.875 {
                (trunc - 1, Self::One)
            } else {
                (trunc - 1, Self::Zero)
            }
        } else {
            #[allow(clippy::collapsible_else_if)]
            if fract < 0.125 {
                (trunc, Self::Zero)
            } else if fract < 0.375 {
                (trunc, Self::One)
            } else if fract < 0.625 {
                (trunc, Self::Two)
            } else if fract < 0.875 {
                (trunc, Self::Three)
            } else {
                (trunc + 1, Self::Zero)
            }
        }
    }

    pub const fn as_float(&self) -> f32 {
        match self {
            Self::Zero => 0.0,
            Self::One => 0.25,
            Self::Two => 0.5,
            Self::Three => 0.75,
            Self::Fine(value) => *value as f32 / 64.0,
        }
    }
}

#[test]
fn native_cache_keys_preserve_fine_positions_without_changing_portable_bins() {
    for (position, native, portable) in [
        (0.1, 0.09375, 0.0),
        (0.9, 0.90625, 1.0),
        (-0.1, -0.09375, 0.0),
        (-0.9, -0.90625, -1.0),
        (1.999, 2.0, 2.0),
    ] {
        let build = |flags| {
            CacheKey::new(
                fontdb::ID::dummy(),
                0,
                13.0,
                (position, position),
                fontdb::Weight::NORMAL,
                flags,
            )
        };
        for (flags, expected) in [
            (CacheKeyFlags::NATIVE_RASTER, native),
            (CacheKeyFlags::empty(), portable),
        ] {
            let (key, x, y) = build(flags);
            assert_eq!(x as f32 + key.x_bin.as_float(), expected);
            assert_eq!(y as f32 + key.y_bin.as_float(), expected);
        }
    }
}

#[test]
fn test_subpixel_bins() {
    // POSITIVE TESTS

    // Maps to 0.0
    assert_eq!(SubpixelBin::new(0.0), (0, SubpixelBin::Zero));
    assert_eq!(SubpixelBin::new(0.124), (0, SubpixelBin::Zero));

    // Maps to 0.25
    assert_eq!(SubpixelBin::new(0.125), (0, SubpixelBin::One));
    assert_eq!(SubpixelBin::new(0.25), (0, SubpixelBin::One));
    assert_eq!(SubpixelBin::new(0.374), (0, SubpixelBin::One));

    // Maps to 0.5
    assert_eq!(SubpixelBin::new(0.375), (0, SubpixelBin::Two));
    assert_eq!(SubpixelBin::new(0.5), (0, SubpixelBin::Two));
    assert_eq!(SubpixelBin::new(0.624), (0, SubpixelBin::Two));

    // Maps to 0.75
    assert_eq!(SubpixelBin::new(0.625), (0, SubpixelBin::Three));
    assert_eq!(SubpixelBin::new(0.75), (0, SubpixelBin::Three));
    assert_eq!(SubpixelBin::new(0.874), (0, SubpixelBin::Three));

    // Maps to 1.0
    assert_eq!(SubpixelBin::new(0.875), (1, SubpixelBin::Zero));
    assert_eq!(SubpixelBin::new(0.999), (1, SubpixelBin::Zero));
    assert_eq!(SubpixelBin::new(1.0), (1, SubpixelBin::Zero));
    assert_eq!(SubpixelBin::new(1.124), (1, SubpixelBin::Zero));

    // NEGATIVE TESTS

    // Maps to 0.0
    assert_eq!(SubpixelBin::new(-0.0), (0, SubpixelBin::Zero));
    assert_eq!(SubpixelBin::new(-0.124), (0, SubpixelBin::Zero));

    // Maps to 0.25
    assert_eq!(SubpixelBin::new(-0.125), (-1, SubpixelBin::Three));
    assert_eq!(SubpixelBin::new(-0.25), (-1, SubpixelBin::Three));
    assert_eq!(SubpixelBin::new(-0.374), (-1, SubpixelBin::Three));

    // Maps to 0.5
    assert_eq!(SubpixelBin::new(-0.375), (-1, SubpixelBin::Two));
    assert_eq!(SubpixelBin::new(-0.5), (-1, SubpixelBin::Two));
    assert_eq!(SubpixelBin::new(-0.624), (-1, SubpixelBin::Two));

    // Maps to 0.75
    assert_eq!(SubpixelBin::new(-0.625), (-1, SubpixelBin::One));
    assert_eq!(SubpixelBin::new(-0.75), (-1, SubpixelBin::One));
    assert_eq!(SubpixelBin::new(-0.874), (-1, SubpixelBin::One));

    // Maps to 1.0
    assert_eq!(SubpixelBin::new(-0.875), (-1, SubpixelBin::Zero));
    assert_eq!(SubpixelBin::new(-0.999), (-1, SubpixelBin::Zero));
    assert_eq!(SubpixelBin::new(-1.0), (-1, SubpixelBin::Zero));
    assert_eq!(SubpixelBin::new(-1.124), (-1, SubpixelBin::Zero));
}
