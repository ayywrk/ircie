use std::{
    any::{Any, TypeId},
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use crate::{factory::Factory, system::SystemParam, IrcContext, IrcPrefix};

#[derive(Debug)]
pub struct Res<'a, T: 'static> {
    value: &'a T,
}

impl<'a, T: 'static> Deref for Res<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'a, T: 'static> AsRef<T> for Res<'a, T> {
    fn as_ref(&self) -> &T {
        self.value
    }
}

impl<'res, T: 'static> SystemParam for Res<'res, T> {
    type Item<'new> = Res<'new, T>;

    fn retrieve<'r>(
        _prefix: &'r IrcPrefix,
        _channel: &str,
        _arguments: &'r [&'r str],
        _context: &'r IrcContext,
        factory: &'r Factory,
    ) -> Self::Item<'r> {
        Res {
            value: &factory
                .resources
                .get(&TypeId::of::<T>())
                .unwrap()
                .downcast_ref()
                .unwrap(),
        }
    }
}

pub struct ResMut<'a, T: 'static> {
    value: &'a mut T,
}

impl<'a, T: 'static> Deref for ResMut<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'a, T: 'static> DerefMut for ResMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<'a, T: 'static> AsRef<T> for ResMut<'a, T> {
    fn as_ref(&self) -> &T {
        self.value
    }
}

impl<'a, T: 'static> AsMut<T> for ResMut<'a, T> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<'res, T: 'static> SystemParam for ResMut<'res, T> {
    type Item<'new> = ResMut<'new, T>;

    fn retrieve<'r>(
        _prefix: &'r IrcPrefix,
        _channel: &str,
        _arguments: &'r [&'r str],
        _context: &'r IrcContext,
        factory: &'r Factory,
    ) -> Self::Item<'r> {
        let const_ptr = &factory.resources as *const HashMap<TypeId, Box<dyn Any + Send + Sync>>;
        let mut_ptr = const_ptr as *mut HashMap<TypeId, Box<dyn Any>>;
        let res_mut = unsafe { &mut *mut_ptr };

        ResMut {
            value: res_mut
                .get_mut(&TypeId::of::<T>())
                .unwrap()
                .downcast_mut()
                .unwrap(),
        }
    }
}

impl<'a> SystemParam for IrcPrefix<'a> {
    type Item<'new> = IrcPrefix<'new>;

    fn retrieve<'r>(
        prefix: &'r IrcPrefix,
        _channel: &str,
        _arguments: &'r [&'r str],
        _context: &'r IrcContext,
        _factory: &'r Factory,
    ) -> Self::Item<'r> {
        prefix.clone()
    }
}

pub struct Channel<'a>(&'a str);
impl<'a> Deref for Channel<'a> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> SystemParam for Channel<'a> {
    type Item<'new> = Channel<'new>;

    fn retrieve<'r>(
        _prefix: &'r IrcPrefix,
        channel: &'r str,
        _arguments: &'r [&'r str],
        _context: &'r IrcContext,
        _factory: &'r Factory,
    ) -> Self::Item<'r> {
        Channel(channel)
    }
}

#[derive(Debug)]
pub struct AnyArguments<'a>(&'a [&'a str]);

impl<'a> Deref for AnyArguments<'a> {
    type Target = &'a [&'a str];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> SystemParam for AnyArguments<'a> {
    type Item<'new> = AnyArguments<'new>;

    fn retrieve<'r>(
        _prefix: &'r IrcPrefix,
        _channel: &str,
        arguments: &'r [&'r str],
        _context: &'r IrcContext,
        _factory: &'r Factory,
    ) -> Self::Item<'r> {
        AnyArguments(&arguments)
    }
}

pub struct Arguments<'a, const N: usize>(&'a [&'a str]);

impl<'a, const N: usize> Deref for Arguments<'a, N> {
    type Target = &'a [&'a str];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a, const N: usize> SystemParam for Arguments<'a, N> {
    type Item<'new> = Arguments<'new, N>;

    fn retrieve<'r>(
        _prefix: &'r IrcPrefix,
        _channel: &str,
        arguments: &'r [&'r str],
        _context: &'r IrcContext,
        _factory: &'r Factory,
    ) -> Self::Item<'r> {
        Arguments(&arguments[..N])
    }

    fn valid(_prefix: &IrcPrefix, arguments: &[&str], _factory: &Factory) -> bool {
        arguments.len() == N
    }
}

pub struct Context<'a>(&'a mut IrcContext);

impl<'a> Deref for Context<'a> {
    type Target = IrcContext;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> DerefMut for Context<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a> SystemParam for Context<'a> {
    type Item<'new> = Context<'new>;

    fn retrieve<'r>(
        _prefix: &'r IrcPrefix,
        _channel: &str,
        _arguments: &'r [&'r str],
        context: &'r IrcContext,
        _factory: &'r Factory,
    ) -> Self::Item<'r> {
        let const_ptr = context as *const IrcContext;
        let mut_ptr = const_ptr as *mut IrcContext;
        let ctx_mut = unsafe { &mut *mut_ptr };
        Context(ctx_mut)
    }
}
