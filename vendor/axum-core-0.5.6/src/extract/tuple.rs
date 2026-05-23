use super::{FromRequest, FromRequestParts, Request};
use crate::response::{IntoResponse, Response};
use http::request::Parts;
use core::marker::{Send, Sync};
use core::result::Result::{self, Ok};
use core::{convert::Infallible, future::Future};

#[diagnostic::do_not_recommend]
impl<S> FromRequestParts<S> for ()
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(_: &mut Parts, _: &S) -> Result<(), Self::Rejection> {
        Ok(())
    }
}

macro_rules! impl_from_request {
    (
        [$($ty:ident),*], $last:ident
    ) => {
        #[diagnostic::do_not_recommend]
        #[allow(non_snake_case, unused_mut, unused_variables)]
        impl<S, $($ty,)* $last> FromRequestParts<S> for ($($ty,)* $last,)
        where
            $( $ty: FromRequestParts<S> + Send, )*
            $last: FromRequestParts<S> + Send,
            S: Send + Sync,
        {
            type Rejection = Response;

            async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
                $(
                    let $ty = $ty::from_request_parts(parts, state)
                        .await
                        .map_err(|err| err.into_response())?;
                )*
                let $last = $last::from_request_parts(parts, state)
                    .await
                    .map_err(|err| err.into_response())?;

                Ok(($($ty,)* $last,))
            }
        }

        // This impl must not be generic over M, otherwise it would conflict with the blanket
        // implementation of `FromRequest<S, Mut>` for `T: FromRequestParts<S>`.
        #[diagnostic::do_not_recommend]
        #[allow(non_snake_case, unused_mut, unused_variables)]
        impl<S, $($ty,)* $last> FromRequest<S> for ($($ty,)* $last,)
        where
            $( $ty: FromRequestParts<S> + Send, )*
            $last: FromRequest<S> + Send,
            S: Send + Sync,
        {
            type Rejection = Response;

            fn from_request(req: Request, state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
                let (mut parts, body) = req.into_parts();

                async move {
                    $(
                        let $ty = $ty::from_request_parts(&mut parts, state).await.map_err(|err| err.into_response())?;
                    )*

                    let req = Request::from_parts(parts, body);

                    let $last = $last::from_request(req, state).await.map_err(|err| err.into_response())?;

                    Ok(($($ty,)* $last,))
                }
            }
        }
    };
}
