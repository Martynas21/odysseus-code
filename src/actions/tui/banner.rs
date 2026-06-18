use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

const BOAT: &[&str] = &[" |\\", " |_\\", "\\____/"];

pub(super) const BOAT_BAND_HEIGHT: u16 = 4;

const CLOUD_SMALL: &[&str] = &[".-~-."];
const CLOUD_BIG: &[&str] = &[".-~-.", "(___)"];

const BIRD_DOWN: char = 'v';
const BIRD_UP: char = '^';
const BIRD_FLAP_RATE: f64 = 5.0;

const WAVE_CADENCE: f64 = 1.2;

const CLOUD_SPEED: f64 = 1.5;
const BIRD_SPEED: f64 = 2.5;

const THINK_SPEEDUP: f64 = 7.0;

pub(super) fn anim_step(phase: f64, dt: f64, thinking: bool) -> f64 {
    let rate = if thinking { THINK_SPEEDUP } else { 1.0 };
    phase + dt.clamp(0.0, 0.25) * rate
}

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

fn bird_glyph(t: f64, phase: f64) -> char {
    if ((t * BIRD_FLAP_RATE + phase) as i64).rem_euclid(2) == 0 {
        BIRD_DOWN
    } else {
        BIRD_UP
    }
}

fn scroll_x(phase: f64, t: f64, speed: f64, span: i32, sprite_w: i32) -> i32 {
    (phase - t * speed).rem_euclid(span as f64) as i32 - sprite_w
}

fn sprite_width(sprite: &[&str]) -> i32 {
    sprite.iter().map(|l| l.chars().count()).max().unwrap_or(0) as i32
}

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

pub(super) fn render_banner(frame: &mut Frame, area: Rect, drift: f64, flap_t: f64) {
    let boat_w = sprite_width(BOAT);
    let boat_h = BOAT.len() as i32;
    if (area.height as i32) <= boat_h || (area.width as i32) < boat_w {
        return;
    }

    let style = Style::new().fg(Color::Blue).add_modifier(Modifier::DIM);

    let width = area.width as i32;
    let sky_top = area.y as i32;
    let water_y = area.bottom() as i32 - 1;
    let buf = frame.buffer_mut();

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

    let base_x = area.x as i32 + (width - boat_w) / 2;
    let base_y = water_y - (boat_h - 1);
    put_sprite(buf, area, BOAT, base_x, base_y, style);

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
