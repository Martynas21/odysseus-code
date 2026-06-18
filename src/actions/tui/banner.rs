//! The decorative boat banner: a parked dinghy on a rippling waterline with
//! drifting clouds and flapping birds, layered back-to-front. The scene drifts
//! constantly and speeds up while a reply is pending.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// A small triangle-sailed dinghy: two rows of sail above a hull that rides the
/// waterline. Spaces are transparent; every other glyph is painted dimmed.
const BOAT: &[&str] = &[" |\\", " |_\\", "\\____/"];

/// Height of the boat band, in rows: one sky row for the clouds and birds, the
/// boat's two sail rows, and the rippling waterline its hull sits on.
pub(super) const BOAT_BAND_HEIGHT: u16 = 4;

/// Clouds drift along the top of the band. The small one is a single row; the
/// big one is two rows, so its underside spills onto the first sail row — and,
/// drawn in front of the boat, hides the sail there. Spaces are transparent.
const CLOUD_SMALL: &[&str] = &[".-~-."];
const CLOUD_BIG: &[&str] = &[".-~-.", "(___)"];

/// Birds flit through the two sail rows behind the boat (the sails paint over,
/// hiding, any that overlap). Each flaps by alternating a down-stroke `v` and an
/// up-stroke `^` at `BIRD_FLAP_RATE` beats per second.
const BIRD_DOWN: char = 'v';
const BIRD_UP: char = '^';
const BIRD_FLAP_RATE: f64 = 5.0;

/// Crest drift of the waterline, in radians of phase per unit drift. A higher
/// cadence packs the crests so the water reads as moving.
const WAVE_CADENCE: f64 = 1.2;

/// Sky drift speeds, in columns per unit drift. Parallax: clouds (far) crawl,
/// birds (nearer) are quicker, and both trail the water.
const CLOUD_SPEED: f64 = 1.5;
const BIRD_SPEED: f64 = 2.5;

/// How much faster the whole scene drifts while a reply is pending. The drift
/// phase is accumulated (see `anim_step`), so this only changes the *rate* — the
/// scene accelerates from its current position rather than jumping.
const THINK_SPEEDUP: f64 = 7.0;

/// Advance the accumulated drift phase by one frame's `dt` seconds, scaled up
/// while `thinking`. `dt` is clamped so a delayed frame can't lurch the scene.
pub(super) fn anim_step(phase: f64, dt: f64, thinking: bool) -> f64 {
    let rate = if thinking { THINK_SPEEDUP } else { 1.0 };
    phase + dt.clamp(0.0, 0.25) * rate
}

/// Glyph of the swell at column `x` and time `t`: `^` crest, `_` trough, `~`
/// between. `cadence` sets how fast crests drift past a fixed column, so a
/// higher cadence makes the water rush by.
fn wave_glyph(x: i32, t: f64, cadence: f64) -> char {
    let v = (x as f64 * 0.30 + t * cadence).sin();
    if v > 0.5 {
        '^'
    } else if v < -0.5 {
        '_'
    } else {
        '~'
    }
}

/// A flapping bird's wing at time `t`, offset by `phase` so a flock doesn't beat
/// in unison: the down-stroke `v` and up-stroke `^` alternate at `BIRD_FLAP_RATE`.
fn bird_glyph(t: f64, phase: f64) -> char {
    if ((t * BIRD_FLAP_RATE + phase) as i64).rem_euclid(2) == 0 {
        BIRD_DOWN
    } else {
        BIRD_UP
    }
}

/// Left-edge column of a sprite that scrolls leftward and wraps. `phase` is its
/// start offset in columns, `t` the elapsed time, `speed` its drift in columns
/// per second, `span` the wrap width (band width + sprite width) and `sprite_w`
/// the sprite's width. The result runs from `-sprite_w` (just off the left) up
/// to `span - sprite_w` (just off the right), so the sprite re-enters smoothly.
fn scroll_x(phase: f64, t: f64, speed: f64, span: i32, sprite_w: i32) -> i32 {
    (phase - t * speed).rem_euclid(span as f64) as i32 - sprite_w
}

/// Width (in display columns) of the widest row of a sprite.
fn sprite_width(sprite: &[&str]) -> i32 {
    sprite.iter().map(|l| l.chars().count()).max().unwrap_or(0) as i32
}

/// Paint a multi-row sprite at `(x, y)`, treating spaces as transparent. Later
/// `put_sprite` calls overpaint earlier ones, so draw order sets the z-order.
fn put_sprite(buf: &mut Buffer, area: Rect, sprite: &[&str], x: i32, y: i32, style: Style) {
    for (ry, line) in sprite.iter().enumerate() {
        for (cx, ch) in line.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            put(buf, area, x + cx as i32, y + ry as i32, ch, style);
        }
    }
}

/// Draw the sky, boat, and rippling waterline into `area` (no border; the band
/// spans the full width). Layered back to front so the z-order reads right:
/// birds (behind the sails) → water → boat → clouds (in front of the sails).
/// The boat stays parked at centre; the water, clouds, and birds scroll by
/// `drift` (accumulated upstream, faster while thinking) so the boat reads as
/// speeding along without moving. Wings flap on the real-time `flap_t` so they
/// stay a steady beat regardless of drift speed. `put` clips outside `area`.
pub(super) fn render_banner(frame: &mut Frame, area: Rect, drift: f64, flap_t: f64) {
    let boat_w = sprite_width(BOAT);
    let boat_h = BOAT.len() as i32;
    // Nothing sensible to draw if the band can't hold the boat and a sky row.
    if (area.height as i32) <= boat_h || (area.width as i32) < boat_w {
        return;
    }

    let style = Style::new().fg(Color::Blue).add_modifier(Modifier::DIM);

    let width = area.width as i32;
    let sky_top = area.y as i32;
    let water_y = area.bottom() as i32 - 1;
    let buf = frame.buffer_mut();

    // Birds first, scattered across the two sail rows, so the boat's sails paint
    // over (hide) any that overlap them.
    let bird_w = 1;
    let bird_span = width + bird_w;
    for (i, &(frac, row)) in [(0.10, 1), (0.38, 2), (0.63, 1), (0.86, 2)]
        .iter()
        .enumerate()
    {
        let x = scroll_x(
            frac * bird_span as f64,
            drift,
            BIRD_SPEED,
            bird_span,
            bird_w,
        );
        put(
            buf,
            area,
            x,
            sky_top + row,
            bird_glyph(flap_t, i as f64),
            style,
        );
    }

    // The waterline: the band's bottom row, rippling across its full width.
    for sx in 0..width {
        let x = area.x as i32 + sx;
        put(
            buf,
            area,
            x,
            water_y,
            wave_glyph(x, drift, WAVE_CADENCE),
            style,
        );
    }

    // The boat, hull on the waterline and sails above, parked at centre and
    // drawn over the birds and swell so neither shows through the solid hull.
    let base_x = area.x as i32 + (width - boat_w) / 2;
    let base_y = water_y - (boat_h - 1);
    put_sprite(buf, area, BOAT, base_x, base_y, style);

    // Clouds last, along the top row, so a big cloud's underside hides the first
    // sail row where it overlaps.
    let cloud_w = sprite_width(CLOUD_BIG);
    let cloud_span = width + cloud_w;
    for &(frac, sprite) in &[(0.00, CLOUD_BIG), (0.45, CLOUD_SMALL), (0.72, CLOUD_BIG)] {
        let x = scroll_x(
            frac * cloud_span as f64,
            drift,
            CLOUD_SPEED,
            cloud_span,
            cloud_w,
        );
        put_sprite(buf, area, sprite, x, sky_top, style);
    }
}

/// Paint a single character if `(x, y)` lies within `bounds`. Out-of-range
/// positions (including negative coordinates from the drift) are ignored.
fn put(buf: &mut Buffer, bounds: Rect, x: i32, y: i32, ch: char, style: Style) {
    if x < bounds.left() as i32 || x >= bounds.right() as i32 {
        return;
    }
    if y < bounds.top() as i32 || y >= bounds.bottom() as i32 {
        return;
    }
    if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
        cell.set_char(ch);
        cell.set_style(style);
    }
}

#[cfg(test)]
mod tests;
