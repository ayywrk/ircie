use std::marker::PhantomData;

use crate::{factory::Factory, format::Msg, IrcPrefix};

pub struct FunctionSystem<Input, F> {
    f: F,
    marker: PhantomData<fn() -> Input>,
}

pub trait System {
    fn run(&mut self, prefix: &IrcPrefix, arguments: &[&str], factory: &mut Factory) -> Response;
}

pub trait IntoSystem<Input> {
    type System: System;

    fn into_system(self) -> Self::System;
}

macro_rules! impl_system {
    (
        $($params:ident),*
    ) => {
        #[allow(non_snake_case)]
        #[allow(unused)]
        impl<F, R: IntoResponse, $($params: SystemParam),*> System for FunctionSystem<($($params,)*), F>
            where
                for<'a, 'b> &'a mut F:
                    FnMut( $($params),* ) -> R +
                    FnMut( $(<$params as SystemParam>::Item<'b>),* ) -> R
        {
            fn run(&mut self, prefix: &IrcPrefix, arguments: &[&str], factory: &mut Factory) -> Response {
                fn call_inner<'a, R: IntoResponse, $($params),*>(
                    mut f: impl FnMut($($params),*) -> R,
                    $($params: $params),*
                ) -> Response {
                    f($($params),*).response()
                }

                $(
                    let $params = $params::retrieve(prefix, arguments, &factory);
                )*

                call_inner(&mut self.f, $($params),*)
            }
        }
    }
}

macro_rules! impl_into_system {
    (
        $($params:ident),*
    ) => {
        impl<F, R: IntoResponse, $($params: SystemParam),*> IntoSystem<($($params,)*)> for F
            where
                for<'a, 'b> &'a mut F:
                    FnMut( $($params),* ) -> R +
                    FnMut( $(<$params as SystemParam>::Item<'b>),* ) -> R
        {
            type System = FunctionSystem<($($params,)*), Self>;

            fn into_system(self) -> Self::System {
                FunctionSystem {
                    f: self,
                    marker: Default::default(),
                }
            }
        }
    }
}

impl_system!();
impl_system!(T1);
impl_system!(T1, T2);
impl_system!(T1, T2, T3);
impl_system!(T1, T2, T3, T4);
impl_system!(T1, T2, T3, T4, T5);
impl_system!(T1, T2, T3, T4, T5, T6);
impl_system!(T1, T2, T3, T4, T5, T6, T7);
impl_system!(T1, T2, T3, T4, T5, T6, T7, T8);

impl_into_system!();
impl_into_system!(T1);
impl_into_system!(T1, T2);
impl_into_system!(T1, T2, T3);
impl_into_system!(T1, T2, T3, T4);
impl_into_system!(T1, T2, T3, T4, T5);
impl_into_system!(T1, T2, T3, T4, T5, T6);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7, T8);

pub(crate) type StoredSystem = Box<dyn for<'a> System + Send + Sync>;

pub(crate) trait SystemParam {
    type Item<'new>;
    fn retrieve<'r>(prefix: &'r IrcPrefix, arguments: &'r[&'r str], factory: &'r Factory) -> Self::Item<'r>;
}

#[derive(Clone)]
pub struct Response(pub Option<Vec<String>>);

pub trait IntoResponse {
    fn response(self) -> Response;
}

impl IntoResponse for () {
    fn response(self) -> Response {
        Response(None)
    }
}

impl IntoResponse for String {
    fn response(self) -> Response {
        Response(Some(vec![self]))
    }
}

impl IntoResponse for &str {
    fn response(self) -> Response {
        Response(Some(vec![self.to_owned()]))
    }
}

impl IntoResponse for Msg {
    fn response(self) -> Response {
        Response(Some(vec![self.to_string()]))
    }
}

impl<O, E> IntoResponse for Result<O, E>
where
    O: IntoResponse,
    E: IntoResponse,
{
    fn response(self) -> Response {
        match self {
            Ok(o) => o.response(),
            Err(e) => e.response(),
        }
    }
}

impl<S> IntoResponse for Option<S>
where
    S: IntoResponse,
{
    fn response(self) -> Response {
        match self {
            Some(s) => s.response(),
            None => Response(None),
        }
    }
}

impl<T: std::fmt::Display> IntoResponse for Vec<T> {
    fn response(self) -> Response {
        Response(Some(
            self.iter().map(|elem| elem.to_string()).collect::<Vec<_>>(),
        ))
    }
}

macro_rules! impl_into_response_for_primitives {
    ($param:ident) => {
        impl IntoResponse for $param {
            fn response(self) -> Response {
                Response(Some(vec![self.to_string()]))
            }
        }
    };
}

impl_into_response_for_primitives!(u8);
impl_into_response_for_primitives!(u16);
impl_into_response_for_primitives!(u32);
impl_into_response_for_primitives!(usize);
impl_into_response_for_primitives!(u64);
impl_into_response_for_primitives!(u128);

impl_into_response_for_primitives!(i8);
impl_into_response_for_primitives!(i16);
impl_into_response_for_primitives!(i32);
impl_into_response_for_primitives!(isize);
impl_into_response_for_primitives!(i64);
impl_into_response_for_primitives!(i128);

impl_into_response_for_primitives!(f32);
impl_into_response_for_primitives!(f64);
