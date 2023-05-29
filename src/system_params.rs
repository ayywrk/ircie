use std::{
    any::{Any, TypeId},
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use crate::{factory::Factory, system::SystemParam, IrcPrefix};

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

    fn retrieve<'r>(_prefix: &'r IrcPrefix, factory: &'r Factory) -> Self::Item<'r> {
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

    fn retrieve<'r>(_prefix: &'r IrcPrefix, factory: &'r Factory) -> Self::Item<'r> {
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

    fn retrieve<'r>(prefix: &'r IrcPrefix, _factory: &'r Factory) -> Self::Item<'r> {
        prefix.clone()
    }
}
