use std::marker::PhantomData;

use crate::{factory::Factory, format::Msg, IrcContext, IrcPrefix};

pub struct FunctionSystem<Input, F> {
    f: F,
    marker: PhantomData<fn() -> Input>,
}

pub trait System {
    fn run(
        &mut self,
        prefix: &IrcPrefix,
        channel: &str,
        arguments: &[&str],
        context: &mut IrcContext,
        factory: &mut Factory,
    ) -> Response;
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
            fn run(&mut self, prefix: &IrcPrefix, channel: &str, arguments: &[&str], context: &mut IrcContext, factory: &mut Factory) -> Response {
                fn call_inner<'a, R: IntoResponse, $($params),*>(
                    mut f: impl FnMut($($params),*) -> R,
                    $($params: $params),*
                ) -> Response {
                    f($($params),*).response()
                }

                $(
                    if !$params::valid(prefix, arguments, &factory) {
                        return Response::InvalidArgument;
                    }

                    let $params = $params::retrieve(prefix, channel, arguments, &context, &factory);

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
impl_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
impl_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
impl_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
impl_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
impl_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13);
impl_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
impl_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15);
impl_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16);

impl_into_system!();
impl_into_system!(T1);
impl_into_system!(T1, T2);
impl_into_system!(T1, T2, T3);
impl_into_system!(T1, T2, T3, T4);
impl_into_system!(T1, T2, T3, T4, T5);
impl_into_system!(T1, T2, T3, T4, T5, T6);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7, T8);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15);
impl_into_system!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16);

pub(crate) type StoredSystem = Box<dyn for<'a> System + Send + Sync>;

pub(crate) trait SystemParam {
    type Item<'new>;
    fn retrieve<'r>(
        prefix: &'r IrcPrefix,
        channel: &'r str,
        arguments: &'r [&'r str],
        context: &'r IrcContext,
        factory: &'r Factory,
    ) -> Self::Item<'r>;
    #[allow(unused_variables)]
    fn valid(prefix: &IrcPrefix, arguments: &[&str], factory: &Factory) -> bool {
        true
    }
}

#[derive(Debug)]
pub struct ResponseData {
    pub(crate) highlight: bool,
    pub(crate) data: Vec<String>,
}

impl Default for ResponseData {
    fn default() -> Self {
        Self {
            highlight: true,
            data: Default::default(),
        }
    }
}

#[derive(Debug)]
pub enum Response {
    Data(ResponseData),
    Empty,
    InvalidArgument,
}

impl Into<Response> for ResponseData {
    fn into(self) -> Response {
        Response::Data(self)
    }
}

pub trait IntoResponse {
    fn response(self) -> Response;
}

impl IntoResponse for ResponseData {
    fn response(self) -> Response {
        self.into()
    }
}

impl<T: IntoResponse> IntoResponse for (bool, T) {
    fn response(self) -> Response {
        let resp = self.1.response();

        match resp {
            Response::Data(data) => ResponseData {
                highlight: self.0,
                data: data.data,
            }
            .into(),
            _ => resp,
        }
    }
}

impl IntoResponse for () {
    fn response(self) -> Response {
        Response::Empty
    }
}

impl IntoResponse for String {
    fn response(self) -> Response {
        ResponseData {
            data: vec![self],
            ..Default::default()
        }
        .into()
    }
}

impl IntoResponse for &str {
    fn response(self) -> Response {
        ResponseData {
            data: vec![self.to_owned()],
            ..Default::default()
        }
        .into()
    }
}

impl IntoResponse for Msg {
    fn response(self) -> Response {
        ResponseData {
            data: vec![self.to_string()],
            ..Default::default()
        }
        .into()
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
            None => Response::Empty,
        }
    }
}

impl<T: std::fmt::Display> IntoResponse for Vec<T> {
    fn response(self) -> Response {
        ResponseData {
            data: self.iter().map(|elem| elem.to_string()).collect::<Vec<_>>(),
            ..Default::default()
        }
        .into()
    }
}

macro_rules! impl_into_response_for_primitives {
    ($param:ident) => {
        impl IntoResponse for $param {
            fn response(self) -> Response {
                ResponseData {
                    data: vec![self.to_string()],
                    ..Default::default()
                }
                .into()
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
