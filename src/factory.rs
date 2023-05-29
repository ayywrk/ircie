use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

#[derive(Default)]
pub struct Factory {
    pub(crate) resources: HashMap<TypeId, Box<dyn Any>>,
}
