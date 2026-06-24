/** ```compile_fail,E0277

use alloc::rc::Rc;

rayon_core::join(|| Rc::new(22), || ()); //~ ERROR

``` */
mod left {}

/** ```compile_fail,E0277

use alloc::rc::Rc;

rayon_core::join(|| (), || Rc::new(23)); //~ ERROR

``` */
mod right {}
