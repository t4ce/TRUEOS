#[cfg(feature = "host-rayon")]
pub use rayon::prelude::*;

#[cfg(not(feature = "host-rayon"))]
pub struct SeqParIter<I> {
    iter: I,
}

#[cfg(not(feature = "host-rayon"))]
impl<I> SeqParIter<I> {
    #[inline]
    pub fn new(iter: I) -> Self {
        Self { iter }
    }
}

#[cfg(not(feature = "host-rayon"))]
impl<I: Iterator> SeqParIter<I> {
    #[inline]
    pub fn map<B, F>(self, f: F) -> SeqParIter<core::iter::Map<I, F>>
    where
        F: FnMut(I::Item) -> B,
    {
        SeqParIter::new(self.iter.map(f))
    }

    #[inline]
    pub fn enumerate(self) -> SeqParIter<core::iter::Enumerate<I>> {
        SeqParIter::new(self.iter.enumerate())
    }

    #[inline]
    pub fn zip<J>(self, other: SeqParIter<J>) -> SeqParIter<core::iter::Zip<I, J>>
    where
        J: Iterator,
    {
        SeqParIter::new(self.iter.zip(other.iter))
    }

    #[inline]
    pub fn for_each<F>(self, f: F)
    where
        F: FnMut(I::Item),
    {
        self.iter.for_each(f);
    }

    #[inline]
    pub fn fold<B, ID, F>(self, identity: ID, fold_op: F) -> SeqParIter<core::iter::Once<B>>
    where
        ID: Fn() -> B,
        F: FnMut(B, I::Item) -> B,
    {
        SeqParIter::new(core::iter::once(self.iter.fold(identity(), fold_op)))
    }

    #[inline]
    pub fn reduce<ID, F>(self, identity: ID, reduce_op: F) -> I::Item
    where
        ID: Fn() -> I::Item,
        F: FnMut(I::Item, I::Item) -> I::Item,
    {
        self.iter.reduce(reduce_op).unwrap_or_else(identity)
    }
}

#[cfg(not(feature = "host-rayon"))]
pub trait IntoParallelIterator {
    type Item;
    type Iter: Iterator<Item = Self::Item>;

    fn into_par_iter(self) -> SeqParIter<Self::Iter>;
}

#[cfg(not(feature = "host-rayon"))]
impl<T> IntoParallelIterator for T
where
    T: IntoIterator,
{
    type Item = T::Item;
    type Iter = T::IntoIter;

    #[inline]
    fn into_par_iter(self) -> SeqParIter<Self::Iter> {
        SeqParIter::new(self.into_iter())
    }
}

#[cfg(not(feature = "host-rayon"))]
pub trait ParallelSliceMut<T> {
    fn par_iter_mut(&mut self) -> SeqParIter<core::slice::IterMut<'_, T>>;
    fn par_chunks_mut(&mut self, chunk_size: usize) -> SeqParIter<core::slice::ChunksMut<'_, T>>;
}

#[cfg(not(feature = "host-rayon"))]
impl<T> ParallelSliceMut<T> for [T] {
    #[inline]
    fn par_iter_mut(&mut self) -> SeqParIter<core::slice::IterMut<'_, T>> {
        SeqParIter::new(self.iter_mut())
    }

    #[inline]
    fn par_chunks_mut(&mut self, chunk_size: usize) -> SeqParIter<core::slice::ChunksMut<'_, T>> {
        SeqParIter::new(self.chunks_mut(chunk_size))
    }
}
