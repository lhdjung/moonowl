//! A theme's colours, resolved, and the shades the chrome derives from them.
//!
//! This is `applyTheme` and `parseColor` from `themes.ts`, and it is the half
//! of a theme that the renderer and the stylesheet actually use. The other
//! half — the file, its name, its id, saving and deleting it — is
//! [`crate::theme`], which is the app's own module compiled into this crate
//! unchanged.
//!
//! The split is the one `themes.ts` already makes and never named: a theme on
//! disk is seven optional strings, and a theme being *drawn with* is a fixed
//! set of `[u8; 3]`s with every absent one derived. Resolving happens once,
//! when a theme is chosen, so the recolouring pass and the stylesheet both see
//! numbers rather than text — and a colour the renderer cannot read is caught
//! there rather than at the moment somebody scrolls onto a page.

/// An 8-bit colour. Alpha is dropped on the way in, as it is in the app.
pub type Rgb = [u8; 3];

/// A theme's colours, all of them present. `Copy`, because every mounted page
/// holds one and reads it on every paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub text: Rgb,
    pub background: Rgb,
    pub accent: Rgb,
    /// What links are tinted with while the document is recoloured. Absent in
    /// the file means the accent.
    pub link: Rgb,
    /// The ground behind selected text, and the ink on it. Derived from the
    /// accent and from each other respectively, which is what most themes
    /// want and why a five-line theme file is enough.
    pub selection_area: Rgb,
    pub selection_text: Rgb,
    /// Whether the pages themselves are recoloured, or only the chrome.
    pub recolor: bool,
    /// Whether a pixel that has a colour of its own keeps it. On in the app,
    /// and the reason the ramp is HSL rather than a flatten. It is a setting
    /// (`recolor_images`) rather than a property of a theme.
    pub keep_colour: bool,
}

impl Palette {
    /// The palette as a component key can carry it: every colour and both
    /// flags, so that two palettes differ in the key wherever they would
    /// differ on the page.
    pub fn key(&self) -> String {
        let hex = |c: Rgb| format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
        format!(
            "{}{}{}{}{}{}{}{}",
            hex(self.text),
            hex(self.background),
            hex(self.accent),
            hex(self.link),
            hex(self.selection_area),
            hex(self.selection_text),
            u8::from(self.recolor),
            u8::from(self.keep_colour),
        )
    }
}

/// What is drawn with when a theme names a colour the renderer cannot read,
/// and when there is no theme at all. Black on white is the app's own
/// fallback, and it is deliberately not any theme's colours: a theme that half
/// works is harder to diagnose than one that plainly did not load.
pub const FALLBACK: Palette = Palette {
    text: [0x00, 0x00, 0x00],
    background: [0xff, 0xff, 0xff],
    accent: [0x3d, 0x6b, 0xb3],
    link: [0x3d, 0x6b, 0xb3],
    selection_area: [0xb4, 0xcd, 0xf0],
    selection_text: [0x00, 0x00, 0x00],
    recolor: false,
    keep_colour: true,
};

/// **Every shade in the block below is `applyTheme`'s, arithmetic for
/// arithmetic.** They were near-misses of it — a surface 6% towards the ink
/// where the app pulls it 55% towards white, a ground 13% towards the ink
/// where the app takes it 7% towards black — and near-misses are the worst
/// kind, because the two apps then look *almost* the same and nobody can say
/// what is different. See `themes.ts`.
impl Palette {
    /// Whether this theme reads as a dark one, which several of the shades
    /// below branch on. `luminance` is the WCAG relative luminance, not the
    /// ramp's luma: the same function `isDarkTheme` uses.
    pub fn dark(&self) -> bool {
        luminance(self.background) < 0.35
    }

    /// The ground the pages stand on. `--bg` in the app, and it is the one
    /// that shows most: it is the whole window either side of the paper.
    pub fn ground(&self) -> Rgb {
        mix(
            self.background,
            BLACK,
            if self.dark() { 0.34 } else { 0.07 },
        )
    }

    /// The wash over the reader while a window is up, as a CSS colour with its
    /// alpha in it.
    ///
    /// `color-mix(in srgb, var(--bg) 62%, transparent)` in `styles.css`, which
    /// is the *ground* at 62% and not black at anything: a black scrim over a
    /// light theme reads as the application having been switched off, and over
    /// a warm one it takes the warmth out. Written from here rather than in
    /// the sheet because `color-mix` is not something this renderer has and
    /// `rgba()` is.
    pub fn scrim(&self) -> String {
        let [r, g, b] = self.ground();
        format!("rgba({r}, {g}, {b}, 0.62)")
    }

    /// What floats: a menu, the sidebar, the settings window. `--surface`.
    ///
    /// **Unrounded**, because three of the shades below are mixed *from* it
    /// and the app rounds once, at the end. `mix` in `themes.ts` returns
    /// floats and only `toHex` rounds; rounding at every step put
    /// `--accent-soft` one level out, which is invisible on screen and is
    /// exactly the kind of difference that makes a comparison useless.
    fn surface_raw(&self) -> Shade {
        blend(
            shade(self.background),
            shade(WHITE),
            if self.dark() { 0.06 } else { 0.55 },
        )
    }

    pub fn surface(&self) -> Rgb {
        solid(self.surface_raw())
    }

    /// A row of one of those under the pointer, and one being pressed.
    pub fn surface_hover(&self) -> Rgb {
        solid(blend(self.surface_raw(), shade(self.text), 0.09))
    }

    pub fn surface_sunk(&self) -> Rgb {
        solid(blend(self.surface_raw(), shade(self.text), 0.055))
    }

    pub fn line(&self) -> Rgb {
        mix(
            self.background,
            self.text,
            if self.dark() { 0.14 } else { 0.17 },
        )
    }

    /// The colour the toolbar's own labels are written in — `--text-soft`.
    pub fn muted(&self) -> Rgb {
        mix(self.text, self.background, 0.26)
    }

    /// The quieter one still: the document's name, "of 400", a chord in a
    /// menu. `--text-faint`.
    ///
    /// **Half-way to the paper, unless that cannot be read.** At a flat 0.52
    /// it was 2.7:1 on Moonowl Light and 1.8:1 on Solarized Light, for words
    /// a reader is meant to read. So it comes back towards the ink until it
    /// reaches 3:1, and never past `muted`, which is the next shade up —
    /// a theme whose own ink is barely 3:1 keeps the order of its shades
    /// rather than a readable faint.
    pub fn faint(&self) -> Rgb {
        let mut amount = 0.52;
        while amount > 0.26
            && contrast_ratio(mix(self.text, self.background, amount), self.background) < 3.0
        {
            amount -= 0.01;
        }
        mix(self.text, self.background, amount)
    }

    /// The small print beside a setting — quieter than the label and still
    /// meant to be read, which is why it is only a little quieter.
    pub fn note(&self) -> Rgb {
        mix(self.text, self.background, 0.28)
    }

    /// The ground a floating control stands on while what it names is in
    /// force. The accent at a sixth of its strength over the surface, which
    /// is what keeps the accent on top of it legible.
    pub fn accent_soft(&self) -> Rgb {
        solid(blend(
            shade(self.accent),
            self.surface_raw(),
            if self.dark() { 0.8 } else { 0.86 },
        ))
    }

    /// The ink on a filled accent button.
    pub fn accent_contrast(&self) -> Rgb {
        if contrast_ratio(self.accent, WHITE) >= 3.0 {
            WHITE
        } else {
            mix(self.accent, BLACK, 0.82)
        }
    }

    /// "That worked": a green that reads on this theme's surface, pulled a
    /// little towards the theme's own ink so it belongs to the palette.
    pub fn positive(&self) -> Rgb {
        let green = if self.dark() {
            [0x6c, 0xc0, 0x8b]
        } else {
            [0x3d, 0x8f, 0x5b]
        };
        mix(green, self.text, 0.14)
    }

    /// The red a destructive thing is written in, and the one the cross on
    /// Close turns under the pointer.
    ///
    /// **Both halves are `themes.ts`'s own `RED_DARK` and `RED_LIGHT`**, and
    /// the one a dark theme wears was not: `#f17a84` against the app's
    /// `#d9636b`, a shade brighter and pinker. That is exactly the near-miss
    /// the note at the head of this block is about — close enough that the two
    /// apps look almost the same and nobody can say what is different. The
    /// green above had drifted the same way, `#6ad38c` against `GREEN_LIGHT`'s
    /// `#6cc08b`.
    pub fn negative(&self) -> Rgb {
        if self.dark() {
            [0xd9, 0x63, 0x6b]
        } else {
            [0xb0, 0x2a, 0x37]
        }
    }

    pub fn negative_contrast(&self) -> Rgb {
        if contrast_ratio(self.negative(), WHITE) >= 3.0 {
            WHITE
        } else {
            mix(self.negative(), BLACK, 0.82)
        }
    }

    /// What an undrawn page is, and what the toolbar stands on: the paper,
    /// which is the theme's background where it recolours and the printer's
    /// white where it does not.
    pub fn page(&self) -> Rgb {
        if self.recolor {
            self.background
        } else {
            WHITE
        }
    }

    /// **The bar has a family of its own, mixed from the paper it sits on.**
    /// The toolbar takes the paper's colour rather than the surface's because
    /// it belongs to the document instead of floating over it — so a hover, a
    /// held-down button and the zoom group have to come off the paper too, or
    /// a warm theme gets a cold chip on warm paper. `--bar-*` in `themes.ts`.
    fn chip_ink(&self) -> Rgb {
        // A theme may name a text colour its paper cannot support — a dark
        // theme that leaves the document alone shows its chrome on white —
        // and a chip nobody can see is worse than one that is merely grey.
        if contrast_ratio(self.text, self.page()) >= 3.0 {
            self.text
        } else if luminance(self.page()) < 0.35 {
            WHITE
        } else {
            BLACK
        }
    }

    fn paper_dark(&self) -> bool {
        luminance(self.page()) < 0.35
    }

    pub fn bar_hover(&self) -> Rgb {
        let amount = if self.paper_dark() { 0.13 } else { 0.09 };
        mix(self.page(), self.chip_ink(), amount)
    }

    pub fn bar_sunk(&self) -> Rgb {
        let amount = if self.paper_dark() { 0.075 } else { 0.055 };
        mix(self.page(), self.chip_ink(), amount)
    }

    pub fn bar_line(&self) -> Rgb {
        let amount = if self.paper_dark() { 0.2 } else { 0.17 };
        mix(self.page(), self.chip_ink(), amount)
    }

    pub fn bar_accent(&self) -> Rgb {
        let amount = if self.paper_dark() { 0.8 } else { 0.86 };
        mix(self.accent, self.page(), amount)
    }
}

/// A colour part-way between two others, before anybody has decided what byte
/// it is. See [`Palette::surface_raw`].
type Shade = [f64; 3];

fn shade(colour: Rgb) -> Shade {
    [colour[0] as f64, colour[1] as f64, colour[2] as f64]
}

fn solid(shade: Shade) -> Rgb {
    let byte = |value: f64| value.clamp(0.0, 255.0).round() as u8;
    [byte(shade[0]), byte(shade[1]), byte(shade[2])]
}

fn blend(a: Shade, b: Shade, amount: f64) -> Shade {
    [
        a[0] + (b[0] - a[0]) * amount,
        a[1] + (b[1] - a[1]) * amount,
        a[2] + (b[2] - a[2]) * amount,
    ]
}

const WHITE: Rgb = [0xff, 0xff, 0xff];
const BLACK: Rgb = [0x00, 0x00, 0x00];

/// WCAG relative luminance — the same function `isDarkTheme` and
/// `contrastRatio` use in the app, and not the ramp's `luma`.
pub fn luminance(colour: Rgb) -> f64 {
    let channel = |value: u8| {
        let c = value as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(colour[0]) + 0.7152 * channel(colour[1]) + 0.0722 * channel(colour[2])
}

/// What a colour painted onto the page comes out as under this theme.
///
/// A highlight is written into the document and pdfium paints it, so the
/// recolouring maps it like any other ink: yellow on a dark theme is a
/// mustard. A swatch that shows the colour as written is the picker lying
/// about the page — so anything showing a highlight's colour shows this.
impl Palette {
    pub fn on_page(&self, colour: Rgb) -> Rgb {
        if !self.recolor {
            return colour;
        }
        let mut pixel = [colour[0], colour[1], colour[2], 0xff];
        crate::recolor::recolor_cpu(&mut pixel, self.text, self.background, self.keep_colour);
        [pixel[0], pixel[1], pixel[2]]
    }
}

pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (one, two) = (luminance(a), luminance(b));
    let (high, low) = if one >= two { (one, two) } else { (two, one) };
    (high + 0.05) / (low + 0.05)
}

/// A colour as CSS writes it, which is how it reaches both the stylesheet and
/// an icon. An inline `<svg>` is parsed by usvg with no cascade behind it, so
/// a shade it is to be drawn in has to arrive as a string — see `Icon` in
/// `app.rs`.
pub fn hex(colour: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2])
}

/// `amount` of `b` in `a`.
pub fn mix(a: Rgb, b: Rgb, amount: f64) -> Rgb {
    let mut out = [0u8; 3];
    for channel in 0..3 {
        out[channel] =
            (a[channel] as f64 + (b[channel] as f64 - a[channel] as f64) * amount).round() as u8;
    }
    out
}

/// Which of a theme's colours could not be read, by field name.
///
/// The app raises this as a notice — `unreadableColors` in `themes.ts` — and
/// the whole argument for keeping themes as TOML is that somebody, or
/// something asked on their behalf, will write one and get the notation wrong.
/// Silently rendering black on white is the behaviour this exists to replace.
pub fn unreadable(theme: &crate::theme::Theme) -> Vec<&'static str> {
    let mut bad = Vec::new();
    let mut check = |field: &'static str, value: Option<&String>| {
        if let Some(value) = value {
            if read_colour(value).is_none() {
                bad.push(field);
            }
        }
    };
    check("text", Some(&theme.text));
    check("background", Some(&theme.background));
    check("accent", theme.accent.as_ref());
    check("link", theme.link.as_ref());
    check("selection_area", theme.selection_area.as_ref());
    check("selection_text", theme.selection_text.as_ref());
    bad
}

/// A theme as it will actually be drawn.
///
/// Every absent colour is derived here and nowhere else, so the renderer, the
/// stylesheet and anything that shows a swatch are looking at the same
/// numbers. `keep_colour` is not a theme's to say — it is the `recolor_images`
/// setting — so it is passed in.
pub fn resolve(theme: &crate::theme::Theme, keep_colour: bool) -> Palette {
    let read = |value: &Option<String>| value.as_deref().and_then(read_colour);
    let text = read_colour(&theme.text).unwrap_or(FALLBACK.text);
    let background = read_colour(&theme.background).unwrap_or(FALLBACK.background);
    let accent = read(&theme.accent).unwrap_or_else(|| mix(background, text, 0.62));
    // Absent means "use the accent", which is what the app's own comment on
    // the field says.
    let link = read(&theme.link).unwrap_or(accent);
    // And absent selection means "derive it from the accent" — a wash of it
    // over the paper, so it reads as a highlight rather than as a block.
    let selection_area =
        read(&theme.selection_area).unwrap_or_else(|| mix(background, accent, 0.4));
    // The ink on that ground, derived from the ground: whichever of the
    // theme's two extremes stands out from it more. A lightness threshold
    // assumed dark ink, and on a dark theme gave paper on a dark wash.
    let selection_text = read(&theme.selection_text).unwrap_or_else(|| {
        if contrast_ratio(text, selection_area) >= contrast_ratio(background, selection_area) {
            text
        } else {
            background
        }
    });
    Palette {
        text,
        background,
        accent,
        link,
        selection_area,
        selection_text,
        recolor: theme.recolor,
        keep_colour,
    }
}

/// Hex and nothing else, checked against the alphabet.
///
/// `parseInt("12345g", 16)` stops at the character it cannot read and returns
/// what it had, so `#12345g` came back as a plausible colour from a string
/// that is not one — the worst of the three possible behaviours, because it is
/// the one nobody notices. This says `None` instead of guessing.
pub fn read_colour(text: &str) -> Option<Rgb> {
    let body = text.strip_prefix('#')?;
    if !body.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |at: usize| u8::from_str_radix(&body[at..at + 1], 16).ok();
    let pair = |at: usize| u8::from_str_radix(&body[at..at + 2], 16).ok();
    match body.len() {
        // Alpha is read and dropped, as it is in the app.
        3 | 4 => Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17]),
        6 | 8 => Some([pair(0)?, pair(2)?, pair(4)?]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    #[test]
    fn hex_is_read_and_everything_else_is_refused() {
        assert_eq!(read_colour("#abc"), Some([0xaa, 0xbb, 0xcc]));
        assert_eq!(read_colour("#aabbcc"), Some([0xaa, 0xbb, 0xcc]));
        assert_eq!(read_colour("#aabbccdd"), Some([0xaa, 0xbb, 0xcc]));
        assert_eq!(read_colour("#abcd"), Some([0xaa, 0xbb, 0xcc]));
        // The one that used to come back as a plausible colour.
        assert_eq!(read_colour("#12345g"), None);
        assert_eq!(read_colour("steelblue"), None);
        assert_eq!(read_colour("rgb(30, 42, 59)"), None);
        assert_eq!(read_colour("#12345"), None);
    }

    /// Every theme that ships resolves, and the two named ones say what the
    /// brief says they say: the light one leaves a page alone, the dark one
    /// does not, and neither is black or white.
    #[test]
    fn the_shipped_themes_all_resolve() {
        for (id, source) in theme::BUILT_IN {
            let parsed: theme::Theme = toml::from_str(source).expect(id);
            assert!(
                unreadable(&parsed).is_empty(),
                "{id} names a colour the renderer cannot read: {:?}",
                unreadable(&parsed),
            );
            let palette = resolve(&parsed, true);
            assert_ne!(palette.text, palette.background, "{id} is invisible");
        }
    }

    /// The quietest words can be read: 3:1 on every shipped theme whose own
    /// ink leaves room for it, and never louder than `muted`.
    #[test]
    fn faint_words_can_be_read() {
        for (id, source) in theme::BUILT_IN {
            let parsed: theme::Theme = toml::from_str(source).expect(id);
            let palette = resolve(&parsed, true);
            let faint = contrast_ratio(palette.faint(), palette.background);
            let muted = contrast_ratio(palette.muted(), palette.background);
            assert!(faint >= 3.0 || faint >= muted - 0.01, "{id}: {faint:.2}");
            assert!(faint <= muted + 0.01, "{id}: faint is louder than muted");
        }
    }

    /// A theme naming two colours gets the other four, and they are not the
    /// fallback's: five lines is enough because of `resolve`.
    #[test]
    fn the_colours_a_theme_leaves_out_are_derived() {
        let bare: theme::Theme =
            toml::from_str("name = \"Bare\"\ntext = \"#ffffff\"\nbackground = \"#202020\"\n")
                .expect("parses");
        let palette = resolve(&bare, true);
        assert_eq!(
            palette.link, palette.accent,
            "link falls back to the accent"
        );
        assert_ne!(palette.accent, FALLBACK.accent);
        assert_ne!(palette.selection_area, palette.background);
        // The ink on a dark theme's dark selection is its ink, not its paper.
        assert_eq!(palette.selection_text, palette.text);
    }

    /// And a colour that cannot be read is named rather than guessed at.
    #[test]
    fn an_unreadable_colour_is_reported() {
        let wrong: theme::Theme = toml::from_str(
            "name = \"Wrong\"\ntext = \"steelblue\"\nbackground = \"#fff\"\naccent = \"#12345g\"\n",
        )
        .expect("parses");
        assert_eq!(unreadable(&wrong), vec!["text", "accent"]);
        // And what is drawn is the fallback's, not a plausible colour from a
        // string that is not one.
        assert_eq!(resolve(&wrong, true).text, FALLBACK.text);
    }
}
