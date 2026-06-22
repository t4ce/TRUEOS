use core::{cmp::Ordering, ops::Range};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortError {
    LenMismatch,
    RangeOutOfBounds,
}

/// Returns the stable sorted index for `input[index]`.
///
/// The rank is computed only by reading `input`: all smaller values count before
/// the candidate, and equal values keep their original order by index.
#[inline]
pub fn stable_rank<T: Ord>(input: &[T], index: usize) -> Option<usize> {
    if index >= input.len() {
        return None;
    }

    let candidate = &input[index];
    let mut rank = 0;

    for (other_index, other) in input.iter().enumerate() {
        match other.cmp(candidate) {
            Ordering::Less => rank += 1,
            Ordering::Equal if other_index < index => rank += 1,
            _ => {}
        }
    }

    Some(rank)
}

/// Computes final stable sorted positions for every source element.
///
/// `positions[source_index]` receives the sorted output index for that element.
/// This is the no-copy form of the rank sort and is safe for `no_std` use.
pub fn stable_rank_positions<T: Ord>(
    input: &[T],
    positions: &mut [usize],
) -> Result<(), SortError> {
    stable_rank_positions_range(input, positions, 0..input.len())
}

/// Computes final stable sorted positions for a source-index range.
///
/// This is the useful worker primitive: every worker may read the full `input`
/// while writing only to its own disjoint `positions[range]` slots.
pub fn stable_rank_positions_range<T: Ord>(
    input: &[T],
    positions: &mut [usize],
    range: Range<usize>,
) -> Result<(), SortError> {
    if positions.len() != input.len() {
        return Err(SortError::LenMismatch);
    }
    if range.start > range.end || range.end > input.len() {
        return Err(SortError::RangeOutOfBounds);
    }

    for index in range {
        positions[index] = stable_rank(input, index).expect("range was checked");
    }

    Ok(())
}

/// Stable rank-sort into `output`.
///
/// The source slice is only read. Each source element writes once to its final
/// stable sorted index in `output`.
pub fn stable_rank_sort_copy<T: Ord + Copy>(
    input: &[T],
    output: &mut [T],
) -> Result<(), SortError> {
    stable_rank_sort_copy_range(input, output, 0..input.len())
}

/// Stable rank-sort a source-index range into `output`.
///
/// Multiple workers can call this with the same read-only `input` and disjoint
/// source ranges. Because stable ranks are unique, their output writes are also
/// disjoint as long as each source index is handled by exactly one worker.
pub fn stable_rank_sort_copy_range<T: Ord + Copy>(
    input: &[T],
    output: &mut [T],
    range: Range<usize>,
) -> Result<(), SortError> {
    if output.len() != input.len() {
        return Err(SortError::LenMismatch);
    }
    if range.start > range.end || range.end > input.len() {
        return Err(SortError::RangeOutOfBounds);
    }

    for index in range {
        let rank = stable_rank(input, index).expect("range was checked");
        output[rank] = input[index];
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_rank_handles_duplicates() {
        let input = [2, 2, 4, 5, 6, 2, 1, 0, 9, 8];
        let mut positions = [usize::MAX; 10];

        stable_rank_positions(&input, &mut positions).unwrap();

        assert_eq!(positions, [2, 3, 5, 6, 7, 4, 1, 0, 9, 8]);
    }

    #[test]
    fn stable_rank_sort_copy_sorts_values() {
        let input = [2, 2, 4, 5, 6, 2, 1, 0, 9, 8];
        let mut output = [0; 10];

        stable_rank_sort_copy(&input, &mut output).unwrap();

        assert_eq!(output, [0, 1, 2, 2, 2, 4, 5, 6, 8, 9]);
    }

    #[test]
    fn range_workers_can_fill_one_output() {
        let input = [2, 2, 4, 5, 6, 2, 1, 0, 9, 8];
        let mut output = [0; 10];

        stable_rank_sort_copy_range(&input, &mut output, 0..4).unwrap();
        stable_rank_sort_copy_range(&input, &mut output, 4..7).unwrap();
        stable_rank_sort_copy_range(&input, &mut output, 7..10).unwrap();

        assert_eq!(output, [0, 1, 2, 2, 2, 4, 5, 6, 8, 9]);
    }

    #[test]
    fn preserves_duplicate_source_order_in_positions() {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        struct Item {
            key: u8,
            tag: u8,
        }

        impl Ord for Item {
            fn cmp(&self, other: &Self) -> core::cmp::Ordering {
                self.key.cmp(&other.key)
            }
        }

        impl PartialOrd for Item {
            fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        let input = [
            Item { key: 2, tag: b'a' },
            Item { key: 2, tag: b'b' },
            Item { key: 1, tag: b'x' },
            Item { key: 2, tag: b'c' },
        ];
        let mut output = [input[0]; 4];

        stable_rank_sort_copy(&input, &mut output).unwrap();

        assert_eq!(output[0].tag, b'x');
        assert_eq!(output[1].tag, b'a');
        assert_eq!(output[2].tag, b'b');
        assert_eq!(output[3].tag, b'c');
    }

    #[test]
    fn reports_bad_lengths_and_ranges() {
        let input = [3, 1, 2];
        let mut output = [0; 2];
        let mut positions = [0; 2];

        assert_eq!(stable_rank_sort_copy(&input, &mut output), Err(SortError::LenMismatch));
        assert_eq!(stable_rank_positions(&input, &mut positions), Err(SortError::LenMismatch));

        let mut output = [0; 3];
        assert_eq!(
            stable_rank_sort_copy_range(&input, &mut output, 2..4),
            Err(SortError::RangeOutOfBounds)
        );
    }
}
