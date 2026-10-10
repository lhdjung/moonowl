// The recolouring ramp, as a compute shader.
//
// A faithful port of `recolorByPixel` in `themes.ts` — the same Rec. 601 luma
// rounded rather than truncated, the same white point, the same HSL room, the
// same fade between the colour floor and the colour ceiling. It reads a page
// texture and writes the recoloured page.
//
// The tables the CPU path builds once per theme are computed per pixel here.
// That is the whole difference, and it is deliberate: a lookup on the GPU is a
// texture fetch and a dependent read, while the arithmetic is a handful of
// instructions on a unit that has nothing else to do. The ramp entries are
// rounded to eight bits anyway, so the two paths still meet at the same value.

// Two vec4s and nothing else, so that there is one possible layout rather than
// a std140 rule to be right about: `w` on the ink carries whether colour is
// kept, and `w` on the paper is spare.
struct Theme {
    // The theme's ink, 0..1, with `keep_colour` in `w`: 1 when a pixel that
    // has a colour of its own keeps it, 0 when everything is flattened onto
    // the ramp, which is what a link and a selected word want.
    text: vec4<f32>,
    // The theme's paper.
    bg: vec4<f32>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var painted: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> theme: Theme;

const WHITE_POINT: f32 = 235.0;
const COLOUR_FLOOR: f32 = 12.0;
const COLOUR_FULL: f32 = 32.0;

// The one conversion the original does implicitly, on every write: a
// `Uint8ClampedArray` clamps to 0..255 and rounds halves to even.
fn to_u8(value: f32) -> f32 {
    let clamped = clamp(value, 0.0, 255.0);
    let floored = floor(clamped);
    let fraction = clamped - floored;
    if (fraction > 0.5) { return floored + 1.0; }
    if (fraction < 0.5) { return floored; }
    // A tie goes to the even neighbour.
    if (floored - 2.0 * floor(floored / 2.0) == 0.0) { return floored; }
    return floored + 1.0;
}

fn to_u8v(value: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(to_u8(value.x), to_u8(value.y), to_u8(value.z));
}

// How much chroma an HSL lightness has room for.
fn room_at(lightness: f32) -> f32 {
    return 255.0 - abs(2.0 * lightness - 255.0);
}

// Where a pixel of this luma lands between the theme's ink and its paper.
fn ramp_at(level: f32) -> vec3<f32> {
    let t = min(round(level * 255.0 / WHITE_POINT), 255.0) / 255.0;
    return to_u8v(theme.text.rgb * 255.0 + (theme.bg.rgb - theme.text.rgb) * 255.0 * t);
}

fn share_of(chroma: f32) -> f32 {
    if (chroma <= COLOUR_FLOOR) { return 0.0; }
    if (chroma >= COLOUR_FULL) { return 1.0; }
    return (chroma - COLOUR_FLOOR) / (COLOUR_FULL - COLOUR_FLOOR);
}

@compute @workgroup_size(8, 8)
fn recolor(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(source);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let at = vec2<i32>(i32(id.x), i32(id.y));
    let pixel = textureLoad(source, at, 0);
    // Eight-bit levels, because every threshold in the original is one.
    let rgb = floor(pixel.rgb * 255.0 + 0.5);

    // Rec. 601 in integers, rounded rather than floored: the white point
    // multiplies any disagreement between the paths by 255/235.
    let level = floor((rgb.r * 77.0 + rgb.g * 151.0 + rgb.b * 28.0 + 128.0) / 256.0);
    let ramp = ramp_at(level);

    let high = max(rgb.r, max(rgb.g, rgb.b));
    let low = min(rgb.r, min(rgb.g, rgb.b));

    var keep = 0.0;
    if (theme.text.w > 0.5 && level < WHITE_POINT) {
        keep = share_of(high - low);
    }

    var out = ramp;
    if (keep > 0.0) {
        // The lightness the ramp asked for, and the chroma that fits there.
        let mapped = floor((max(ramp.r, max(ramp.g, ramp.b)) + min(ramp.r, min(ramp.g, ramp.b))) / 2.0);
        let here = room_at(floor((high + low) / 2.0));
        var inverse = 0.0;
        if (here != 0.0) { inverse = 1.0 / here; }
        let scale = room_at(mapped) * inverse;
        let foot = mapped - (high - low) * scale / 2.0;
        out = to_u8v(ramp + (foot + (rgb - low) * scale - ramp) * keep);
    }

    textureStore(painted, at, vec4<f32>(out / 255.0, pixel.a));
}
