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
    // `w` on the ink is -1, or, when a printed colour counts as ink at full
    // strength (`Region::colour_is_ink`), the least channel that counts as
    // paper: the level is the least channel measured against it.
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
    // The runs lie on shelves (`stack` in gpu.rs): in the order of the
    // array, each shelf's top is at or below the last one's, and along a
    // shelf each run starts to the right of the one before. No two runs
    // share a cell of the grid, so the run under a cell is found by two
    // binary searches instead of a test against every run. A page with two
    // hundred links costs eight steps a cell, not two hundred.
    let count = u32(how_many.count.x);
    let x = f32(id.x);
    let y = f32(id.y);

    // The shelf: the last run whose top is at or above this cell.
    var low = 0u;
    var high = count;
    while (low < high) {
        let middle = (low + high) / 2u;
        if (runs[middle].span.y <= y) { low = middle + 1u; } else { high = middle; }
    }
    if (low == 0u) {
        return;
    }
    let shelf = runs[low - 1u].span.y;

    // Along it: the last run that starts at or left of this cell. Every run
    // on an earlier shelf counts as "before", so the search can start at 0.
    var end = low;
    low = 0u;
    while (low < end) {
        let middle = (low + end) / 2u;
        let before = runs[middle];
        if (before.span.y < shelf || before.span.x <= x) { low = middle + 1u; } else { end = middle; }
    }
    if (low == 0u) {
        return;
    }
    let run = runs[low - 1u];
    // The ragged edges: right of a shelf's last run, and under a run shorter
    // than the shelf.
    if (run.span.y != shelf || x >= run.span.x + run.span.z || y >= run.span.y + run.span.w) {
        return;
    }
    let along = vec2<f32>(x - run.span.x, y - run.span.y);
    let write = run.places.xy + along;
    let read = run.places.zw + along;
    let ink = run.ink.rgb;
    let ground = run.ink.w;
    let paper = run.paper.rgb;

    let pixel = textureLoad(untouched, vec2<i32>(i32(read.x), i32(read.y)), 0);
    let rgb = floor(pixel.rgb * 255.0 + 0.5);
    // Rec. 601 in integers, rounded rather than floored: the white point
    // multiplies any disagreement between the paths by 255/235.
    var level = floor((rgb.r * 77.0 + rgb.g * 151.0 + rgb.b * 28.0 + 128.0) / 256.0);
    // `stretched` in recolor.rs, in whole numbers, which f32 holds exactly.
    if (ground >= 0.0) {
        let top = max(ground, 1.0);
        let least = min(rgb.r, min(rgb.g, rgb.b));
        level = min(floor((least * 255.0 + floor(top / 2.0)) / top), 255.0);
    }
    let t = min(round(level * 255.0 / WHITE_POINT), 255.0) / 255.0;
    let out = ink * 255.0 + (paper - ink) * 255.0 * t;
    let eight = vec3<f32>(to_u8(out.x), to_u8(out.y), to_u8(out.z));
    textureStore(painted, vec2<i32>(i32(write.x), i32(write.y)), vec4<f32>(eight / 255.0, pixel.a));
}
