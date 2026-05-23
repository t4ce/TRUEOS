#[allow(unused_imports)]
use crate::runtime::prelude::*;

use crate::runtime::task;


pub(crate) trait Overflow<T: 'static> {
    fn push(&self, task: task::Notified<T>);

    fn push_batch<I>(&self, iter: I)
    where
        I: Iterator<Item = task::Notified<T>>;
}

