//! Sub-cell-precise horizontal bars (Unicode eighth blocks) and the header
//! sparkline. Pure functions returning styled spans.

use ratatui::style::Style;
use ratatui::text::Span;

use crate::theme::{self, Rgb, Stops, Theme};

/// Left-aligned partial blocks, indexed by eighths filled (0–8).
const EIGHTHS: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const TRACK: char = '·';

#[derive(Debug, Clone, Copy)]
pub enum Fill {
    /// Colour each cell by its position along the whole bar.
    Gradient(&'static Stops),
    Solid(Rgb),
}

/// One stacked part of a bar.
#[derive(Debug, Clone, Copy)]
pub struct Segment {
    /// Fraction of the whole bar, `0.0..=1.0`.
    pub frac: f32,
    pub fill: Fill,
    /// Full-cell character in `NO_COLOR` mode, so stacked segments stay
    /// distinguishable without colour (e.g. `█` used, `░` cache).
    pub mono: char,
}

impl Segment {
    pub fn new(frac: f32, fill: Fill) -> Self {
        Segment {
            frac,
            fill,
            mono: '█',
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Cell {
    Track,
    Full(usize),
    /// `eighths` of segment `left`, the remainder showing `right`.
    Partial {
        left: usize,
        eighths: usize,
        right: Option<usize>,
    },
}

/// Lays out segments over `width` cells.
fn cells(width: usize, segments: &[Segment]) -> Vec<Cell> {
    const EPS: f32 = 1e-4;
    let w = width as f32;
    let mut end = 0.0f32;
    let ends: Vec<f32> = segments
        .iter()
        .map(|s| {
            let f = if s.frac.is_finite() {
                s.frac.max(0.0)
            } else {
                0.0
            };
            end = (end + f * w).min(w);
            end
        })
        .collect();
    // The segment covering position x (skipping zero-width ones), if any.
    let at = |x: f32| ends.iter().position(|&e| e > x + EPS);

    (0..width)
        .map(|i| {
            let x0 = i as f32;
            let full = |seg: Option<usize>| seg.map_or(Cell::Track, Cell::Full);
            let Some(left) = at(x0) else {
                return Cell::Track;
            };
            let e = ends[left];
            if e >= x0 + 1.0 - EPS {
                return Cell::Full(left);
            }
            let right = at(e);
            match ((e - x0) * 8.0).round() as usize {
                0 => full(right),
                8.. => Cell::Full(left),
                eighths => Cell::Partial {
                    left,
                    eighths,
                    right,
                },
            }
        })
        .collect()
}

/// Renders a stacked bar exactly `width` cells wide.
pub fn bar(theme: &Theme, width: usize, segments: &[Segment]) -> Vec<Span<'static>> {
    let pos = |i: usize| (i as f32 + 0.5) / width.max(1) as f32;
    let color = |seg: usize, i: usize| match segments[seg].fill {
        Fill::Gradient(stops) => theme::gradient(stops, pos(i)),
        Fill::Solid(c) => c,
    };
    let mut out: Vec<Span<'static>> = Vec::with_capacity(width);
    for (i, cell) in cells(width, segments).into_iter().enumerate() {
        let (ch, style) = match cell {
            Cell::Track => (TRACK, theme.track()),
            Cell::Full(seg) if theme.mono() => (segments[seg].mono, Style::new()),
            Cell::Full(seg) => ('█', theme.fg(color(seg, i))),
            Cell::Partial { left, eighths, .. } if theme.mono() => {
                // Without colour, a partial `█`-segment keeps its eighth
                // block; a `░`-style segment can only be shown whole.
                let ch = if segments[left].mono == '█' {
                    EIGHTHS[eighths]
                } else {
                    segments[left].mono
                };
                (ch, Style::new())
            }
            Cell::Partial {
                left,
                eighths,
                right,
            } => {
                let mut style = theme.fg(color(left, i));
                if let Some(bg) = right.and_then(|r| theme.color(color(r, i))) {
                    style = style.bg(bg);
                }
                (EIGHTHS[eighths], style)
            }
        };
        push(&mut out, ch, style);
    }
    out
}

/// Appends a character, merging into the previous span when styles match.
fn push(out: &mut Vec<Span<'static>>, ch: char, style: Style) {
    if let Some(last) = out.last_mut()
        && last.style == style
    {
        last.content.to_mut().push(ch);
        return;
    }
    out.push(Span::styled(ch.to_string(), style));
}

/// The most recent `width` values (each `0.0..=1.0`) as a coloured
/// sparkline. Shorter histories produce shorter output.
pub fn sparkline<'a>(
    theme: &Theme,
    values: impl DoubleEndedIterator<Item = &'a f32> + ExactSizeIterator,
    width: usize,
) -> Vec<Span<'static>> {
    let skip = values.len().saturating_sub(width);
    let mut out = Vec::new();
    for &v in values.skip(skip) {
        let v = if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let ch = SPARK[((v * 7.0).round() as usize).min(7)];
        push(&mut out, ch, theme.usage(v));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{CACHE, ColorMode, USAGE};

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn mono() -> Theme {
        Theme::new(ColorMode::Mono)
    }

    fn one(frac: f32, width: usize) -> String {
        text(&bar(
            &mono(),
            width,
            &[Segment::new(frac, Fill::Gradient(USAGE))],
        ))
    }

    #[test]
    fn whole_cells() {
        assert_eq!(one(0.0, 4), "····");
        assert_eq!(one(0.5, 4), "██··");
        assert_eq!(one(1.0, 4), "████");
        assert_eq!(one(7.0, 4), "████");
        assert_eq!(one(-1.0, 4), "····");
        assert_eq!(one(f32::NAN, 4), "····");
    }

    #[test]
    fn eighth_precision() {
        // 10 cells: 0.05 = half a cell = 4 eighths.
        assert_eq!(one(0.05, 10), "▌·········");
        assert_eq!(one(0.0125, 10), "▏·········");
        assert_eq!(one(0.2875, 10), "██▉·······");
        // Less than 1/16 of a cell rounds down to nothing.
        assert_eq!(one(0.005, 10), "··········");
        for frac in [0.0, 0.123, 0.5, 0.999, 1.0] {
            assert_eq!(one(frac, 37).chars().count(), 37);
        }
    }

    #[test]
    fn stacked_segments_in_mono() {
        let segs = [
            Segment::new(0.33, Fill::Gradient(USAGE)),
            Segment {
                frac: 0.25,
                fill: Fill::Solid(CACHE),
                mono: '░',
            },
        ];
        // used ends at 3.3 cells (▎), cache at 5.8 (shown whole in mono).
        assert_eq!(text(&bar(&mono(), 10, &segs)), "███▎░░···\u{b7}");
    }

    #[test]
    fn colored_partial_uses_next_segment_as_background() {
        let t = Theme::new(ColorMode::TrueColor);
        let segs = [
            Segment::new(0.15, Fill::Solid(Rgb(1, 2, 3))),
            Segment::new(0.5, Fill::Solid(Rgb(4, 5, 6))),
        ];
        let spans = bar(&t, 10, &segs);
        let partial = spans.iter().find(|s| s.content == "▌").unwrap();
        assert_eq!(partial.style.fg, t.color(Rgb(1, 2, 3)));
        assert_eq!(partial.style.bg, t.color(Rgb(4, 5, 6)));
    }

    #[test]
    fn sparkline_levels_and_width() {
        let values = [0.0f32, 0.5, 1.0, 2.0];
        assert_eq!(text(&sparkline(&mono(), values.iter(), 10)), "▁▅██");
        // Only the most recent values fit.
        assert_eq!(text(&sparkline(&mono(), values.iter(), 2)), "██");
        assert_eq!(text(&sparkline(&mono(), [].iter(), 5)), "");
    }
}
