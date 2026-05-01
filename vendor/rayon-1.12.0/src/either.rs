/// Minimal in-tree replacement for the `either` crate's `Either`.
///
/// TRUEOS vendors Rayon as kernel-facing source, so keeping this tiny sum type
/// local avoids carrying an extra fundamental crate for one enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Either<L, R> {
    /// Left-hand variant.
    Left(L),
    /// Right-hand variant.
    Right(R),
}

impl<L, R> Either<L, R> {
    /// Borrow the contained value, preserving the side.
    #[inline]
    pub fn as_ref(&self) -> Either<&L, &R> {
        match self {
            Either::Left(left) => Either::Left(left),
            Either::Right(right) => Either::Right(right),
        }
    }

    /// Mutably borrow the contained value, preserving the side.
    #[inline]
    pub fn as_mut(&mut self) -> Either<&mut L, &mut R> {
        match self {
            Either::Left(left) => Either::Left(left),
            Either::Right(right) => Either::Right(right),
        }
    }

    /// Apply one of two functions depending on the contained side.
    #[inline]
    pub fn either<T>(self, left: impl FnOnce(L) -> T, right: impl FnOnce(R) -> T) -> T {
        match self {
            Either::Left(value) => left(value),
            Either::Right(value) => right(value),
        }
    }
}

impl<L, R> Iterator for Either<L, R>
where
    L: Iterator,
    R: Iterator<Item = L::Item>,
{
    type Item = L::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Either::Left(iter) => iter.next(),
            Either::Right(iter) => iter.next(),
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Either::Left(iter) => iter.size_hint(),
            Either::Right(iter) => iter.size_hint(),
        }
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        match self {
            Either::Left(iter) => iter.nth(n),
            Either::Right(iter) => iter.nth(n),
        }
    }
}

impl<L, R> DoubleEndedIterator for Either<L, R>
where
    L: DoubleEndedIterator,
    R: DoubleEndedIterator<Item = L::Item>,
{
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Either::Left(iter) => iter.next_back(),
            Either::Right(iter) => iter.next_back(),
        }
    }
}

impl<L, R> ExactSizeIterator for Either<L, R>
where
    L: ExactSizeIterator,
    R: ExactSizeIterator<Item = L::Item>,
{
    #[inline]
    fn len(&self) -> usize {
        match self {
            Either::Left(iter) => iter.len(),
            Either::Right(iter) => iter.len(),
        }
    }
}
