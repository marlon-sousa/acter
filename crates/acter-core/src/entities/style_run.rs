//! Entity/value: how a span of a line's text is drawn, as the far end styled it.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind")]
pub enum Colour {
    /// One of the sixteen palette colours, 0 to 15.
    Named {
        index: u8,
    },
    /// An entry of the 256-colour table from 16 to 255.
    Indexed {
        index: u8,
    },
    Rgb {
        r: u8,
        g: u8,
        b: u8,
    },
}

/// A `None` colour is the terminal's own default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
pub struct Style {
    pub fg: Option<Colour>,
    pub bg: Option<Colour>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub strike: bool,
}

/// `start` and `len` count UTF-16 code units of the line's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StyleRun {
    pub start: u32,
    pub len: u32,
    pub style: Style,
}

impl StyleRun {
    fn end(&self) -> u32 {
        self.start + self.len
    }
}

/// The runs over `from..to` of a line, counted from `from`.
pub fn slice_runs(runs: &[StyleRun], from: u32, to: u32) -> Vec<StyleRun> {
    runs.iter()
        .filter_map(|run| {
            let start = run.start.max(from);
            let end = run.end().min(to);
            (start < end).then(|| StyleRun {
                start: start - from,
                len: end - start,
                style: run.style,
            })
        })
        .collect()
}

/// Adds the runs of text appended after `before`, merging a run that continues the last one.
pub fn join_runs(runs: &mut Vec<StyleRun>, before: &str, later: &[StyleRun]) {
    let offset = utf16_len(before);
    for run in later {
        let start = offset + run.start;
        match runs.last_mut() {
            Some(last) if last.style == run.style && last.end() == start => last.len += run.len,
            _ => runs.push(StyleRun { start, ..*run }),
        }
    }
}

pub(crate) fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const RED: Style = Style {
        fg: Some(Colour::Named { index: 1 }),
        bg: None,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
        inverse: false,
        strike: false,
    };

    const BOLD: Style = Style { bold: true, ..RED };

    fn run(start: u32, len: u32, style: Style) -> StyleRun {
        StyleRun { start, len, style }
    }

    #[test]
    fn a_slice_keeps_the_part_of_each_run_inside_it_counted_from_its_start() {
        let runs = [run(0, 3, RED), run(5, 4, BOLD)];

        assert_eq!(
            slice_runs(&runs, 2, 7),
            vec![run(0, 1, RED), run(3, 2, BOLD)]
        );
        assert_eq!(slice_runs(&runs, 3, 5), vec![]);
    }

    #[test]
    fn joined_runs_are_shifted_by_the_earlier_text_in_utf16_units() {
        let mut runs = vec![run(0, 1, BOLD)];

        join_runs(&mut runs, "a😀", &[run(1, 2, RED)]);

        assert_eq!(runs, vec![run(0, 1, BOLD), run(4, 2, RED)]);
    }

    #[test]
    fn a_joined_run_that_continues_the_last_one_merges_with_it() {
        let mut runs = vec![run(0, 2, RED)];

        join_runs(&mut runs, "ab", &[run(0, 3, RED), run(3, 1, BOLD)]);

        assert_eq!(runs, vec![run(0, 5, RED), run(5, 1, BOLD)]);
    }

    #[test]
    fn a_run_serializes_with_its_colour_tagged_by_kind() {
        let styled = run(
            2,
            3,
            Style {
                bg: Some(Colour::Rgb { r: 1, g: 2, b: 3 }),
                ..RED
            },
        );

        assert_eq!(
            serde_json::to_value(styled).unwrap(),
            json!({
                "start": 2,
                "len": 3,
                "style": {
                    "fg": { "kind": "Named", "index": 1 },
                    "bg": { "kind": "Rgb", "r": 1, "g": 2, "b": 3 },
                    "bold": false,
                    "dim": false,
                    "italic": false,
                    "underline": false,
                    "inverse": false,
                    "strike": false,
                },
            })
        );
    }
}
