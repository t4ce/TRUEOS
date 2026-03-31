use core::ops::Deref;

use spin::Once;
pub use spin::RwLock;

pub struct LazyLock<T> {
    once: Once<T>,
    init: fn() -> T,
}

impl<T> LazyLock<T> {
    pub const fn new(init: fn() -> T) -> Self {
        Self {
            once: Once::new(),
            init,
        }
    }

    pub fn force(this: &Self) -> &T {
        this.once.call_once(this.init)
    }
}

impl<T> Deref for LazyLock<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        Self::force(self)
    }
}

impl<T> Default for LazyLock<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::new(T::default)
    }
}
