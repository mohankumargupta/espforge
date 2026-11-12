//! When may two manifests name one crate?
//!
//! A driver manifest names a crate with a Cargo version requirement (`0.8.1`,
//! `^0.8`, `>=0.8, <0.9`). Two drivers that both want `embedded-graphics` are
//! compatible when one version can satisfy both, which is the question Cargo
//! itself answers at resolution time. Comparing the text refuses `0.8.1`
//! beside `0.8.2` — two spellings of one caret window.
//!
//! This answers only "can a solution exist", never "which version": Cargo
//! resolves, and the composed manifest keeps the stricter requirement. The
//! window is a release triple comparison, so a pre-release is read as its
//! release (`1.0.0-rc.1` is not older than `1.0.0`); no driver manifest names
//! a pre-release, and reading one as older would invent a conflict.

use semver::{Comparator, Op, VersionReq};
use std::cmp::Ordering;

/// A release triple.
type Point = (u64, u64, u64);

/// One end of a window: the point, and whether the window includes it.
type Bound = (Point, bool);

/// The versions a requirement allows: from `low` up to `high`, with no
/// ceiling when `high` is `None`.
#[derive(Clone, Copy)]
struct Window {
    low: Bound,
    high: Option<Bound>,
}

impl Window {
    const EVERYTHING: Window = Window {
        low: ((0, 0, 0), true),
        high: None,
    };

    fn at_least(self, point: Point, inclusive: bool) -> Window {
        Window {
            low: higher(self.low, (point, inclusive)),
            high: self.high,
        }
    }

    fn below(self, point: Point, inclusive: bool) -> Window {
        Window {
            low: self.low,
            high: Some(match self.high {
                None => (point, inclusive),
                Some(high) => lower(high, (point, inclusive)),
            }),
        }
    }

    /// The versions both windows allow: the higher floor, the lower ceiling.
    fn and(self, other: Window) -> Window {
        Window {
            low: higher(self.low, other.low),
            high: match (self.high, other.high) {
                (None, high) => high,
                (high, None) => high,
                (Some(left), Some(right)) => Some(lower(left, right)),
            },
        }
    }

    fn is_empty(self) -> bool {
        match self.high {
            None => false,
            Some((high, high_inclusive)) => match self.low.0.cmp(&high) {
                Ordering::Less => false,
                Ordering::Greater => true,
                // One point: allowed only when both ends include it.
                Ordering::Equal => !(self.low.1 && high_inclusive),
            },
        }
    }
}

/// The stricter of two floors: the later one, inclusive only if both are.
fn higher(left: Bound, right: Bound) -> Bound {
    match left.0.cmp(&right.0) {
        Ordering::Greater => left,
        Ordering::Less => right,
        Ordering::Equal => (left.0, left.1 && right.1),
    }
}

/// The tighter of two ceilings: the earlier one, inclusive only if both are.
fn lower(left: Bound, right: Bound) -> Bound {
    match left.0.cmp(&right.0) {
        Ordering::Less => left,
        Ordering::Greater => right,
        Ordering::Equal => (left.0, left.1 && right.1),
    }
}

/// The window one comparator of a requirement allows.
fn comparator_window(comparator: &Comparator) -> Window {
    let point = (
        comparator.major,
        comparator.minor.unwrap_or(0),
        comparator.patch.unwrap_or(0),
    );
    match comparator.op {
        Op::Exact => Window::EVERYTHING.at_least(point, true).below(point, true),
        Op::Greater => Window::EVERYTHING.at_least(point, false),
        Op::GreaterEq => Window::EVERYTHING.at_least(point, true),
        Op::Less => Window::EVERYTHING.below(point, false),
        Op::LessEq => Window::EVERYTHING.below(point, true),
        Op::Tilde => Window::EVERYTHING
            .at_least(point, true)
            .below(next_minor(comparator), false),
        Op::Caret => Window::EVERYTHING
            .at_least(point, true)
            .below(next_breaking(comparator), false),
        Op::Wildcard => Window::EVERYTHING
            .at_least(point, true)
            .below(next_minor(comparator), false),
        // `Op` is non-exhaustive; an operator this crate does not know cannot
        // be produced by `VersionReq::parse`, so it narrows nothing.
        _ => Window::EVERYTHING,
    }
}

/// `~1.2.3` ends at `1.3.0`; `~1` and `1.*` end at `2.0.0`.
fn next_minor(comparator: &Comparator) -> Point {
    match comparator.minor {
        Some(minor) => (comparator.major, minor + 1, 0),
        None => (comparator.major + 1, 0, 0),
    }
}

/// The first version a caret requirement no longer allows: `1.2.3` and `1.2`
/// end at `2.0.0`, `0.8.3` and `0.8` at `0.9.0`, `0.0.3` at `0.0.4`.
fn next_breaking(comparator: &Comparator) -> Point {
    let major = comparator.major;
    let minor = comparator.minor.unwrap_or(0);
    let patch = comparator.patch.unwrap_or(0);
    if major > 0 {
        (major + 1, 0, 0)
    } else if minor > 0 {
        (0, minor + 1, 0)
    } else {
        (0, 0, patch + 1)
    }
}

/// Every comparator at once: a requirement's commas mean "and".
fn window(requirement: &VersionReq) -> Window {
    requirement
        .comparators
        .iter()
        .fold(Window::EVERYTHING, |window, comparator| {
            window.and(comparator_window(comparator))
        })
}

/// Whether one version can satisfy both requirements.
///
/// A requirement that is not a Cargo range (`"latest"`) is compared as text,
/// so a typo stays a conflict instead of being unified with something else.
pub fn overlap(left: &str, right: &str) -> bool {
    match (VersionReq::parse(left), VersionReq::parse(right)) {
        (Ok(left), Ok(right)) => !window(&left).and(window(&right)).is_empty(),
        _ => left == right,
    }
}

/// Which of two overlapping requirements to keep: the higher floor, then the
/// lower ceiling, then the exclusive end. `left` wins a tie, so a merged
/// manifest keeps the first manifest's spelling.
pub fn stricter<'a>(left: &'a str, right: &'a str) -> &'a str {
    match (VersionReq::parse(left), VersionReq::parse(right)) {
        (Ok(left_requirement), Ok(right_requirement)) => {
            let left_window = window(&left_requirement);
            let right_window = window(&right_requirement);
            if is_tighter(&right_window, &left_window) {
                right
            } else {
                left
            }
        }
        _ => left,
    }
}

/// Whether `candidate` allows fewer versions than `other`.
fn is_tighter(candidate: &Window, other: &Window) -> bool {
    match candidate.low.0.cmp(&other.low.0) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => {
            if candidate.low.1 != other.low.1 {
                // An exclusive floor excludes a version an inclusive one keeps.
                return !candidate.low.1;
            }
            match (candidate.high, other.high) {
                (Some((candidate, candidate_inclusive)), Some((other, other_inclusive))) => {
                    match candidate.cmp(&other) {
                        Ordering::Less => true,
                        Ordering::Greater => false,
                        Ordering::Equal => !candidate_inclusive && other_inclusive,
                    }
                }
                (Some(_), None) => true,
                _ => false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_versions_inside_one_caret_window_overlap() {
        assert!(overlap("0.8.1", "0.8.2"));
        assert!(overlap("0.8", "0.8.5"));
        assert!(overlap("1.2.3", "1.9.0"));
        assert!(overlap("0.8.1", "^0.8"));
    }

    #[test]
    fn a_new_breaking_version_does_not() {
        assert!(!overlap("0.8.1", "0.9.0"));
        assert!(!overlap("1.0.0", "2.0.0"));
        // `0.0.x` bumps the patch, so `0.0.3` and `0.0.4` are incompatible.
        assert!(!overlap("0.0.3", "0.0.4"));
    }

    #[test]
    fn explicit_ranges_compare_by_their_bounds() {
        assert!(overlap(">=0.8, <0.9", "0.8.3"));
        assert!(!overlap(">=0.9", "0.8"));
        assert!(overlap("~0.8.1", "0.8.5"));
        assert!(!overlap("~0.8.1", "0.9.0"));
        assert!(overlap("0.8.*", "0.8.2"));
        assert!(!overlap("0.8.*", "0.9.0"));
        assert!(overlap("*", "0.8.1"));
    }

    #[test]
    fn an_exact_requirement_allows_one_version() {
        assert!(overlap("=0.8.1", "0.8.1"));
        assert!(!overlap("=0.8.1", "0.8.2"));
    }

    #[test]
    fn an_unparseable_requirement_compares_as_text() {
        assert!(overlap("latest", "latest"));
        assert!(!overlap("latest", "0.8"));
    }

    #[test]
    fn the_stricter_requirement_is_the_one_to_keep() {
        assert_eq!(stricter("0.8.1", "0.8.2"), "0.8.2");
        assert_eq!(stricter("0.8.2", "0.8.1"), "0.8.2");
        assert_eq!(stricter("0.8", "0.8.4"), "0.8.4");
        // A tie keeps the existing spelling.
        assert_eq!(stricter("0.8.1", "^0.8.1"), "0.8.1");
        // A ceiling is a requirement too.
        assert_eq!(stricter(">=0.8", ">=0.8, <0.9"), ">=0.8, <0.9");
    }
}
