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
    /// The ground the pages stand on: the window either side of the paper,
    /// between pages, and the start screen. `--bg` in the app. Absent in the
    /// file means the background, a little darker.
    pub ground: Rgb,
    /// Whether the pages themselves are recoloured, or only the chrome.
    pub recolor: bool,
    /// Whether a pixel that has a colour of its own keeps it. On in the app,
    /// and the reason the ramp is HSL rather than a flatten. It is a setting
    /// (`recolor_images`) rather than a property of a theme.
    pub keep_colour: bool,
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
    ground: [0xed, 0xed, 0xed],
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
        let [r, g, b] = self.ground;
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

    /// A rule or a border, on the background and on the surface alike — the
    /// two are near enough on some dark themes (Solarized Dark's were 1.03:1)
    /// that a flat mix drew a line on one and nothing on the other.
    pub fn line(&self) -> Rgb {
        let mut amount = if self.dark() { 0.14 } else { 0.17 };
        let seen = |colour| {
            contrast_ratio(colour, self.background).min(contrast_ratio(colour, self.surface()))
        };
        while amount < 0.4 && seen(mix(self.background, self.text, amount)) < 1.2 {
            amount += 0.01;
        }
        mix(self.background, self.text, amount)
    }

    /// The contrast a chrome shade has on the worst of what it is written on:
    /// the background, a menu's surface, and the ground of the start screen.
    fn worst(&self, colour: Rgb) -> f64 {
        [self.background, self.surface(), self.ground]
            .into_iter()
            .map(|under| contrast_ratio(colour, under))
            .fold(f64::INFINITY, f64::min)
    }

    /// How far from the ink towards the paper a shade can go and still read
    /// at `target` on all of it — from `start`, never nearer the ink than
    /// `floor`, so the shades keep their order on a theme whose own ink is low.
    fn readable(&self, start: f64, floor: f64, target: f64) -> f64 {
        let mut amount = start;
        while amount > floor && self.worst(mix(self.text, self.background, amount)) < target {
            amount -= 0.01;
        }
        amount.max(floor)
    }

    fn muted_amount(&self) -> f64 {
        self.readable(0.26, 0.12, 4.5)
    }

    /// The colour the toolbar's own labels are written in — `--text-soft`.
    pub fn muted(&self) -> Rgb {
        mix(self.text, self.background, self.muted_amount())
    }

    /// The quieter one still: the document's name, "of 400", a chord in a
    /// menu. `--text-faint`.
    ///
    /// **Half-way to the paper, unless that cannot be read.** At a flat 0.52
    /// it was 2.7:1 on Moonowl Light and 1.8:1 on Solarized Light, for words
    /// a reader is meant to read. So it comes back towards the ink until it
    /// reaches 4.5:1 on everything it is written on — the bar for small text,
    /// which is what it is used for: 3:1 left thumbnail numbers and the chords
    /// on the Keyboard page hard to make out — and never past `muted`, the
    /// next shade up. A theme whose own ink is barely that keeps the order of
    /// its shades rather than a readable faint.
    pub fn faint(&self) -> Rgb {
        let amount = self.readable(0.52, self.muted_amount(), 4.5);
        mix(self.text, self.background, amount)
    }

    /// The small print beside a setting — quieter than the label and still
    /// meant to be read, which is why it is only a little quieter. Walked
    /// back to 4.5:1 as [`Palette::faint`] is: at a flat 0.28 it was 2.2:1 on
    /// Solarized Light, for every explanatory sentence in Settings.
    pub fn note(&self) -> Rgb {
        let amount = self.readable(0.28, self.muted_amount(), 4.5);
        mix(self.text, self.background, amount)
    }

    /// `colour`, moved away from `grounds` — towards white on a dark one,
    /// black on a light one — until it reads at `target` on every one of
    /// them, but only by a nudge. Where a nudge is not enough, the theme's own
    /// ink: **never a colour the theme does not name.** Moved as far as it
    /// took, a mid-tone theme's accent came out white and its red brown. Not towards the theme's ink: Tokyo Night
    /// Storm's accent is as light as its ink, and moving to it got nowhere.
    fn away_from(&self, colour: Rgb, grounds: &[Rgb], target: f64) -> Rgb {
        let pole = if luminance(grounds[0]) < 0.35 {
            WHITE
        } else {
            BLACK
        };
        let worst = |colour| {
            grounds
                .iter()
                .map(|&under| contrast_ratio(colour, under))
                .fold(f64::INFINITY, f64::min)
        };
        let mut amount: f64 = 0.0;
        while amount < NUDGE && worst(mix(colour, pole, amount)) < target {
            amount += 0.02;
        }
        let nudged = mix(colour, pole, amount.min(NUDGE));
        if worst(nudged) >= target {
            nudged
        } else {
            self.text
        }
    }

    /// The accent as the words on its own tint are written: the selected tab,
    /// the current outline row, a chip that is on. The accent itself fell
    /// under 3:1 on its tint on three shipped themes.
    pub fn accent_ink(&self) -> Rgb {
        self.away_from(self.accent, &[self.accent_soft(), self.bar_accent()], 4.5)
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

    /// The ink on a filled accent button: white or a near-black of the
    /// accent's own, whichever reads better on it. White at 3:1 left "Save
    /// theme" at 3.7:1 on both Solarized themes.
    pub fn accent_contrast(&self) -> Rgb {
        on_fill(self.accent)
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
    ///
    /// Moved towards the ink where it does not reach 3:1 on the grounds it is
    /// drawn on — it was 2.8:1 on Glamour's sunk bar, for the one
    /// destructive control in the highlight popover.
    pub fn negative(&self) -> Rgb {
        // By the surface it is drawn on rather than by the paper: a mid-tone
        // theme can be dark by its paper and light by its surface, and the
        // dark theme's red darkened to read there was brown.
        let red = if luminance(self.surface()) < 0.35 {
            [0xd9, 0x63, 0x6b]
        } else {
            [0xb0, 0x2a, 0x37]
        };
        self.away_from(red, &[self.surface(), self.bar_sunk()], 3.0)
    }

    /// The ink on a filled red button, chosen as the accent's is: on a dark
    /// theme's red that is the near-black, which white reaches only 3.5:1 on.
    pub fn negative_contrast(&self) -> Rgb {
        on_fill(self.negative())
    }

    /// What an undrawn page is: the paper, which is the theme's background
    /// where it recolours and the printer's white where it does not.
    pub fn page(&self) -> Rgb {
        if self.recolor {
            self.background
        } else {
            WHITE
        }
    }

    /// **The bar has a family of its own, mixed from the background it
    /// stands on** — the theme's own, even where the pages stay the printer's
    /// white, so Moonowl Light's bar is its cream rather than a strip of
    /// paper. A hover, a held-down button and the zoom group come off that
    /// background rather than the surface, or a warm theme gets a cold chip on
    /// a warm bar. `--bar-*` in `themes.ts`.
    ///
    /// **A field on the bar is the ground**, the colour around the pages, as
    /// a row of the start screen is the bar's colour on the ground: the pair
    /// of shades the theme already has, where ink mixed into the background
    /// was a grey on a warm theme. Ink is mixed in where the ground is not a
    /// quiet step from the background: where it cannot be told from it, or
    /// where it is a world away, as a mid-tone theme's derived one is.
    pub fn bar_sunk(&self) -> Rgb {
        if (1.05..=1.4).contains(&contrast_ratio(self.ground, self.background)) {
            self.ground
        } else {
            let amount = if self.dark() { 0.075 } else { 0.055 };
            mix(self.background, self.text, amount)
        }
    }

    /// A hover on the bar: on a light theme the field's shade, the ground, as
    /// a row of the start screen takes the bar's; on a dark one a lift towards
    /// the ink, because there the ground is a well and a hover sunk into it
    /// read as black. Inside the zoom group, which is the field's shade, a
    /// hover is the bar's own colour (`.zoom-group .chip:hover`).
    pub fn bar_hover(&self) -> Rgb {
        if self.dark() {
            mix(self.background, self.text, 0.12)
        } else {
            self.bar_sunk()
        }
    }

    /// A rule on the bar, and the edge of what stands on it: further along
    /// the same way again, so a line is the theme's own deeper shade and not
    /// a grey.
    pub fn bar_line(&self) -> Rgb {
        mix(self.background, self.bar_sunk(), 2.2)
    }

    pub fn bar_accent(&self) -> Rgb {
        let amount = if self.dark() { 0.8 } else { 0.86 };
        mix(self.accent, self.background, amount)
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

/// How far [`Palette::away_from`] moves a colour towards white or black
/// before it gives up on it for the theme's own ink. Glamour's accent, the
/// furthest any shipped theme needs, goes 0.48 of the way.
const NUDGE: f64 = 0.5;

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

/// What a highlight's colour comes out as on the page under this theme.
///
/// A highlight is written into the document and pdfium paints it, so the
/// recolouring maps it like any other ink: yellow on a dark theme is a
/// mustard. A swatch that shows the colour as written is the picker lying
/// about the page — so anything showing a highlight's colour shows this.
impl Palette {
    /// **And then the words on it made to read, at 7:1.** The recolouring
    /// puts a highlight between the theme's ink and paper by its lightness,
    /// and a mid-tone mark under the theme's ink reads at no better than
    /// 4.5:1 whichever way it is moved. So the words on a mark take whichever
    /// of the theme's ink and paper stands further off it — black or white
    /// type, on a plain page — and the mark is then moved away from them until
    /// the two read at 7:1, or as well as ink on paper does where that is
    /// less. The page paints both: see `PageWidget::mark_ramp`.
    pub fn marked(&self, colour: Rgb) -> (Rgb, Rgb) {
        let ground = self.drawn(colour);
        let (paper, ink) = if self.recolor {
            (self.background, self.text)
        } else {
            (WHITE, BLACK)
        };
        let words = if contrast_ratio(ground, paper) > contrast_ratio(ground, ink) {
            paper
        } else {
            ink
        };
        let ground = lift(ground, words, contrast_ratio(paper, ink).min(7.0));
        (ground, words)
    }

    /// The ground of [`Palette::marked`]: the colour anything showing a
    /// highlight shows.
    pub fn on_page(&self, colour: Rgb) -> Rgb {
        self.marked(colour).0
    }

    /// The colour as pdfium's mark comes out of the recolouring, before the
    /// page lifts it to [`Palette::on_page`].
    pub fn drawn(&self, colour: Rgb) -> Rgb {
        if !self.recolor {
            return colour;
        }
        let mut pixel = [colour[0], colour[1], colour[2], 0xff];
        crate::recolor::recolor_cpu(&mut pixel, self.text, self.background, self.keep_colour);
        [pixel[0], pixel[1], pixel[2]]
    }
}

/// `ground` moved away from `ink` — towards white under dark ink, black
/// under light — until the two are `target` apart. A ground that *is* the
/// ink, a black mark under black type, goes the way there is room.
fn lift(ground: Rgb, ink: Rgb, target: f64) -> Rgb {
    let darker = if ground == ink {
        contrast_ratio(ink, WHITE) > contrast_ratio(ink, BLACK)
    } else {
        luminance(ink) < luminance(ground)
    };
    let pole = if darker { WHITE } else { BLACK };
    let mut amount = 0.0;
    while amount < 1.0 && contrast_ratio(mix(ground, pole, amount), ink) < target {
        amount += 0.02;
    }
    mix(ground, pole, amount.min(1.0))
}

/// **A highlight's colour as it goes into the file**: light enough that black
/// type under it still stands off it at 3:1. Every reader multiplies a
/// highlight into the page, pdfium included, so under a black mark the words
/// are gone, in Preview and Acrobat too, and no repainting can bring back
/// what the pixels no longer hold. 3:1 is where the glyphs come back whole
/// and, not by chance, where white type reads at 7:1 — so a dark mark is
/// shown as written, with white words: see [`Palette::marked`].
pub fn legible(colour: Rgb) -> Rgb {
    lift(colour, BLACK, 3.0)
}

/// One of the six colours as the swatches offer it: [`legible`], or the text
/// unchanged where it is no colour at all.
pub fn offered(text: &str) -> String {
    read_colour(text).map_or_else(|| text.to_string(), |rgb| hex(legible(rgb)))
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
    check("ground", theme.ground.as_ref());
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
    let mut palette = Palette {
        text,
        background,
        accent,
        link,
        selection_area,
        selection_text,
        ground: background,
        recolor: theme.recolor,
        keep_colour,
    };
    palette.ground = read(&theme.ground)
        .unwrap_or_else(|| mix(background, BLACK, if palette.dark() { 0.34 } else { 0.07 }));
    palette
}

/// The ink for words on a filled button of `fill`.
fn on_fill(fill: Rgb) -> Rgb {
    let dark = mix(fill, BLACK, 0.82);
    if contrast_ratio(fill, WHITE) >= contrast_ratio(fill, dark) {
        WHITE
    } else {
        dark
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
            // A page of text, and the words on a filled button, read at 4.5:1.
            let body = contrast_ratio(palette.text, palette.background);
            assert!(body >= 4.5, "{id}'s text is {body:.2}:1 on its paper");
            let button = contrast_ratio(palette.accent_contrast(), palette.accent);
            assert!(button >= 4.5, "{id}'s button text is {button:.2}:1");
            let danger = contrast_ratio(palette.negative_contrast(), palette.negative());
            assert!(danger >= 4.5, "{id}'s red button text is {danger:.2}:1");
        }
    }

    /// The quietest words can be read: 4.5:1 on the background, a menu and the
    /// start screen of every shipped theme whose own ink leaves room for it,
    /// and never louder than `muted`. And a line is a line on all of them.
    #[test]
    fn faint_words_can_be_read() {
        for (id, source) in theme::BUILT_IN {
            let parsed: theme::Theme = toml::from_str(source).expect(id);
            let palette = resolve(&parsed, true);
            let faint = palette.worst(palette.faint());
            let muted = palette.worst(palette.muted());
            assert!(faint >= 4.5 || faint >= muted - 0.01, "{id}: {faint:.2}");
            assert!(faint <= muted + 0.01, "{id}: faint is louder than muted");
            let note = palette.worst(palette.note());
            assert!(note >= 4.5 || note >= muted - 0.01, "{id}: note {note:.2}");
            let on = |ink, grounds: &[Rgb]| {
                grounds
                    .iter()
                    .map(|&under| contrast_ratio(ink, under))
                    .fold(f64::INFINITY, f64::min)
            };
            let chosen = on(
                palette.accent_ink(),
                &[palette.accent_soft(), palette.bar_accent()],
            );
            assert!(chosen >= 4.5, "{id}: the accent on its tint is {chosen:.2}");
            let red = on(palette.negative(), &[palette.surface(), palette.bar_sunk()]);
            assert!(red >= 3.0, "{id}: the red is {red:.2}");
            let line = palette.line();
            let seen = contrast_ratio(line, palette.surface())
                .min(contrast_ratio(line, palette.background));
            assert!(seen >= 1.2, "{id}: a line nobody can see, {seen:.2}");
        }
    }

    /// **Words under a highlight read as well as words on the paper**, up to
    /// 7:1, in the six defaults and in dark and mid-tone marks — black, a
    /// saturated blue, a grey, a brown, as they go into the file — on every
    /// shipped theme and on a purple one of a reader's own, whose purple
    /// mark was 2.7:1 under its purple ink.
    #[test]
    fn a_highlight_keeps_its_words_readable() {
        let purple = "name = \"Purple\"\ntext = \"#7d34b5\"\nbackground = \"#d7bed7\"\n";
        let sources = theme::BUILT_IN.iter().map(|(_, source)| *source);
        for source in sources.chain([purple]) {
            let theme: theme::Theme = toml::from_str(source).expect("parses");
            for keep_colour in [false, true] {
                let palette = resolve(&theme, keep_colour);
                let (paper, ink) = if palette.recolor {
                    (palette.background, palette.text)
                } else {
                    (WHITE, BLACK)
                };
                let target = contrast_ratio(paper, ink).min(7.0) - 0.05;
                for value in [
                    "#ffd60a", "#7bed9f", "#ff6b6b", "#74c0fc", "#ffa94d", "#da77f2", "#000000",
                    "#0a25ff", "#5961ff", "#777777", "#3c3024", "#7a7068", "#808080",
                ] {
                    let (ground, words) = palette.marked(legible(read_colour(value).unwrap()));
                    let ratio = contrast_ratio(ground, words);
                    assert!(ratio >= target, "{}: {value} at {ratio:.2}", theme.name);
                }
            }
        }
    }

    /// **A mid-tone theme gets nothing it did not name.** A purple theme of
    /// a reader's own, dark by its paper and light by its surface, had its
    /// selected nav item come out white and its Delete button brown.
    #[test]
    fn a_mid_tone_theme_keeps_to_its_own_colours() {
        let purple: theme::Theme = toml::from_str(
            "name = \"Purple\"\ntext = \"#7d34b5\"\nbackground = \"#ca81cb\"\naccent = \"#a549b1\"\n",
        )
        .expect("parses");
        let palette = resolve(&purple, true);
        assert_eq!(palette.accent_ink(), palette.text);
        let [r, g, b] = palette.negative();
        assert!(
            r as f64 > 2.5 * g.max(b) as f64,
            "a red, not a brown: {r} {g} {b}"
        );
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

    /// The ground is derived unless the theme names one, and then it is that.
    #[test]
    fn a_theme_may_name_its_ground() {
        let source = "name = \"G\"\ntext = \"#ffffff\"\nbackground = \"#202020\"\n";
        let bare: theme::Theme = toml::from_str(source).expect("parses");
        assert_eq!(resolve(&bare, true).ground, mix([0x20; 3], BLACK, 0.34));
        let named: theme::Theme =
            toml::from_str(&format!("{source}ground = \"#2a1f3d\"\n")).expect("parses");
        assert_eq!(resolve(&named, true).ground, [0x2a, 0x1f, 0x3d]);
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
