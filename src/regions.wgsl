// The parts of a page that are painted through a ramp of their own.
//
// **Three things in the app want this and all three are saying the same
// thing** — *this part of the page is different*, in a colour the theme chose.
// A link (`tintLinks`), a passage the reader has marked (`tintMarkup`) and a
// word they have swept over (`paintSelection`). What they have in common is
// that the colour the page was printed in is exactly what must not survive: a
// link that keeps the blue it was printed in says nothing. So this is
// `duotone` in `themes.ts` — the ramp before it learned to keep a colour.
//
// **A pass of its own rather than a branch in `recolor.wgsl`.** The obvious
// arrangement hands the recolouring pass a list of rectangles and asks every
// pixel whether it is in one, and a bibliography page has a couple of hundred
// links: two hundred rectangle tests against ten million pixels, for work
// that touches perhaps a fiftieth of the page. So the dispatch is over the
// *runs* instead, stacked one above another into a grid that is the sum of
// their areas and nothing else — which is the same economy the app gets from
// a clip, arrived at the only way a compute pass can.
//
// **And it reads from wherever the untouched pixels are.** A link is tinted
// from the page as pdfium drew it, which is still on the GPU while the page
// is being uploaded — the pristine copy `tintLinks` takes, without the copy.
// A selection is ramped from the page *as shown*, because a recoloured dark
// page is already light ink on dark paper and the selection has to sit on
// what the reader can see; there the untouched pixels are a backup taken
// before the ramp went on. Each run carries where to read as well as where to
// write, which is the whole of the difference.

struct Runs {
    // How many of `runs` are real, in `x`. A storage array cannot be empty
    // and `arrayLength` would answer for the padding too, so the count is
    // carried rather than measured.
    count: vec4<f32>,
};

struct Run {
    // Where this run sits in the dispatch grid: left, top, width, height.
    span: vec4<f32>,
    // Where its top left is on the page, and where its top left is in the
    // texture being read — the same point for a link, and the run's place in
    // the backup for a selection. Not called `from`, which WGSL reserves.
    places: vec4<f32>,
    // What the darkest pixel in it becomes, and what the lightest becomes;
    // `w` on the ink is 1 when a printed colour counts as ink at full
    // strength (`Region::colour_is_ink`): the level is the least channel.
    ink: vec4<f32>,
    paper: vec4<f32>,
};

@group(0) @binding(0) var untouched: texture_2d<f32>;
@group(0) @binding(1) var painted: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> how_many: Runs;
@group(0) @binding(3) var<storage, read> runs: array<Run>;

const WHITE_POINT: f32 = 235.0;

// `Uint8ClampedArray`'s own conversion, which is what the app writes through:
// clamp to 0..255 and round halves to even. Repeated from `recolor.wgsl`
// rather than shared, because a WGSL module is compiled whole and these are
// two pipelines.
fn to_u8(value: f32) -> f32 {
    let clamped = clamp(value, 0.0, 255.0);
    let floored = floor(clamped);
    let fraction = clamped - floored;
    if (fraction > 0.5) { return floored + 1.0; }
    if (fraction < 0.5) { return floored; }
    if (floored - 2.0 * floor(floored / 2.0) == 0.0) { return floored; }
    return floored + 1.0;
}

@compute @workgroup_size(8, 8)
fn ramp_runs(@builtin(global_invocation_id) id: vec3<u32>) {
    // The grid is as wide as the widest run and as tall as all of them, so
    // most of it is a run and the rest is the ragged edge beside the short
    // ones. Later runs win, which is `viewer.ts`'s own order: markup first
    // and links over it, because where a reader has marked a cross-reference
    // the link's colour is the one that should carry.
    let count = u32(how_many.count.x);
    let x = f32(id.x);
    let y = f32(id.y);
    var found = false;
    var read = vec2<f32>(0.0);
    var write = vec2<f32>(0.0);
    var ink = vec3<f32>(0.0);
    var colour_is_ink = false;
    var paper = vec3<f32>(0.0);
    for (var index: u32 = 0u; index < count; index = index + 1u) {
        let run = runs[index];
        if (x >= run.span.x && x < run.span.x + run.span.z
            && y >= run.span.y && y < run.span.y + run.span.w) {
            let along = vec2<f32>(x - run.span.x, y - run.span.y);
            write = run.places.xy + along;
            read = run.places.zw + along;
            ink = run.ink.rgb;
            colour_is_ink = run.ink.w > 0.5;
            paper = run.paper.rgb;
            found = true;
        }
    }
    if (!found) {
        return;
    }

    let pixel = textureLoad(untouched, vec2<i32>(i32(read.x), i32(read.y)), 0);
    let rgb = floor(pixel.rgb * 255.0 + 0.5);
    // Rec. 601 in integers, rounded rather than floored: the white point
    // multiplies any disagreement between the paths by 255/235.
    var level = floor((rgb.r * 77.0 + rgb.g * 151.0 + rgb.b * 28.0 + 128.0) / 256.0);
    if (colour_is_ink) {
        level = min(rgb.r, min(rgb.g, rgb.b));
    }
    let t = min(round(level * 255.0 / WHITE_POINT), 255.0) / 255.0;
    let out = ink * 255.0 + (paper - ink) * 255.0 * t;
    let eight = vec3<f32>(to_u8(out.x), to_u8(out.y), to_u8(out.z));
    textureStore(painted, vec2<i32>(i32(write.x), i32(write.y)), vec4<f32>(eight / 255.0, pixel.a));
}
