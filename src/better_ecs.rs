// Code heavily based on (but not copied from) recs:
// https://github.com/AndyBarron/rustic-ecs

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::cell::{self, RefCell};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use super::util::RefCellTryReplaceExt;

type IdNumber = u64;
type ComponentMap = HashMap<TypeId, ComponentId>;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct EntityId(IdNumber);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ComponentId(IdNumber);

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EcsError {
    EntityNotFound(EntityId),
    ComponentNotFound(ComponentId),
    ComponentTypeNotFound(EntityId),
    ComponentTypeMismatch(ComponentId),
    BorrowError(ComponentId),
    InternalError(&'static str, Option<Box<EcsError>>)
}

struct ComponentEntry {
    pub refbox: RefCell<Box<Any>>,
    pub parent: EntityId,
    pub type_id: TypeId
}

impl ComponentEntry {
    pub fn new<T: Component>(component: T, parent: EntityId) -> Self {
        ComponentEntry {
            refbox: RefCell::new(Box::new(component)),
            parent,
            type_id: TypeId::of::<T>()
        }
    }
}

pub struct Ecs {
    next_entity_id: IdNumber,
    next_component_id: IdNumber,
    entities: HashMap<EntityId, ComponentMap>,
    components: HashMap<ComponentId, ComponentEntry>,
}

pub trait Component: 'static {}

impl<T: 'static> Component for T {}

impl Ecs {
    pub fn new() -> Self {
        Ecs {
            next_entity_id: 0,
            next_component_id: 0,
            entities: HashMap::new(),
            components: HashMap::new(),
        }
    }

    pub fn try_create_entity(&mut self) -> Option<EntityId> {
        let new_id_number = self.next_entity_id;
        self.next_entity_id = self.next_entity_id.checked_add(1)?;
        let new_id = EntityId(new_id_number);

        self.entities.insert(new_id, HashMap::new());

        Some(new_id)
    }

    pub fn create_entity(&mut self) -> EntityId {
        self.try_create_entity().unwrap()
    }

    // Note: Does not touch the entities map.
    fn create_component<T: Component>(&mut self, component: T, parent: EntityId) -> ComponentId {
        let new_id_number = self.next_component_id;
        self.next_component_id = self.next_component_id.checked_add(1).unwrap();
        let new_id = ComponentId(new_id_number);

        self.components
            .insert(new_id, ComponentEntry::new(component, parent));

        new_id
    }

    // Note: Does not touch the entities map.
    // Inverse of create_component.
    pub fn remove_entity(&mut self, entity: EntityId) -> Result<(), EcsError> {
        let components = match self.entities.remove(&entity) {
            Some(components) => components,
            None => return Err(EcsError::EntityNotFound(entity)),
        };

        for (_, id) in components {
            // TODO: This is an internal error; make an error message for it.
            self.components.remove(&id).unwrap();
        }

        Ok(())
    }

    fn remove_component(&mut self, component: ComponentId) -> Result<(), EcsError> {
        match self.components.remove(&component) {
            Some(_) => Ok(()),
            None => Err(EcsError::ComponentNotFound(component)),
        }
    }

    pub fn has_entity(&self, entity: EntityId) -> bool {
        self.entities.contains_key(&entity)
    }

    pub fn has_component<T: Component>(&self, entity: EntityId) -> Result<Option<ComponentId>, EcsError> {
        let components = self.entities.get(&entity)
            .ok_or(EcsError::EntityNotFound(entity))?;
        Ok(components.get(&TypeId::of::<T>()).map(|&id| id))
    }

    pub fn has_component_by_id(&self, entity: EntityId, component: ComponentId) -> Result<bool, EcsError> {
        let components = self.entities.get(&entity)
            .ok_or(EcsError::EntityNotFound(entity))?;
        Ok(components.iter().filter(|(_, &id)| id == component).count() > 0)
    }

    // Note: This will force the new component to have a different EntityId than the old one.
    fn create_and_attach_component<T: Component>(&mut self, entity: EntityId, component: T) -> Result<ComponentId, EcsError> {
        let component_id = self.create_component(component, entity);
        let entity_components = self.entities.get_mut(&entity)
            .ok_or(EcsError::EntityNotFound(entity))?;

        let maybe_old_id = entity_components.insert(TypeId::of::<T>(), component_id);
        if let Some(old_id) = maybe_old_id {
            self.remove_component(old_id)
                .map_err(|e| EcsError::InternalError("Failed to remove old component.", Some(Box::new(e))))?;
        }

        Ok(component_id)
    }

    pub fn replace<T: Component>(&self, entity: EntityId, component: T) -> Result<T, EcsError> {
        if let Some(component_id) = self.has_component::<T>(entity)? {
            let boxed_any = self.get_refcell(component_id)?.try_replace(Box::new(component))
                .map_err(|_| EcsError::BorrowError(component_id))?;

            boxed_any.downcast::<T>()
                .map(|boxed_t| *boxed_t)
                .map_err(|_| panic!("Typecheck succeded and then failed! Ecs left in inconsistent state!"))
        } else {
            Err(EcsError::ComponentTypeNotFound(entity))
        }
    }

    pub fn set<T: Component>(
        &mut self,
        entity: EntityId,
        component: T,
    ) -> Result<ComponentId, EcsError> {
        
        match self.has_component::<T>(entity)? {
            Some(component_id) => {
                // Update that component.
                self.replace::<T>(entity, component)
                    .map(|_| component_id)
            },
            None => self.create_and_attach_component(entity, component)
                .map_err(|e| if let EcsError::EntityNotFound(_) = e {
                        EcsError::InternalError("Couldn't find entity the 2nd time after finding it the 1st.", Some(Box::new(e)))
                    } else {
                        e
                    })
        }
    }

    pub fn lookup_component<T: Component>(&self, entity: EntityId) -> Result<ComponentId, EcsError> {
        let entity_components = match self.entities.get(&entity) {
            Some(v) => v,
            None => return Err(EcsError::EntityNotFound(entity)),
        };

        entity_components.get(&TypeId::of::<T>())
            .map(|&id| id)
            .ok_or(EcsError::ComponentTypeNotFound(entity))
    }

    fn get_refcell(&self, id: ComponentId) -> Result<&RefCell<Box<Any>>, EcsError> {
        self.components.get(&id)
            .map(|v| &v.refbox)
            .ok_or(EcsError::InternalError("Component attached to entity couldn't be found.", None))
    }

    pub fn borrow<T: Component>(&self, entity: EntityId) -> Result<Ref<T>, EcsError> {
        let id = self.lookup_component::<T>(entity)?;
        let refcell = self.get_refcell(id)?;
        let refbox = refcell.try_borrow().map_err(|_| EcsError::BorrowError(id))?;
        Ref::new(refbox).ok_or(EcsError::ComponentTypeMismatch(id))
    }

    pub fn get<T: Component + Clone>(&self, entity: EntityId) -> Result<T, EcsError> {
        self.borrow(entity).map(|c: Ref<T>| c.clone())
    }

    pub fn borrow_mut<T: Component>(&self, entity: EntityId) -> Result<RefMut<T>, EcsError> {
        let id = self.lookup_component::<T>(entity)?;
        let refcell = self.get_refcell(id)?;        
        let refbox = refcell.try_borrow_mut().map_err(|_| EcsError::BorrowError(id))?;
        RefMut::new(refbox).ok_or(EcsError::ComponentTypeMismatch(id))
    }

    pub fn borrow_by_id<T: Component>(&self, component_id: ComponentId) -> Result<Ref<T>, EcsError> {
        let component = self.get_refcell(component_id)?;

        Ref::new(component.try_borrow().map_err(|_| EcsError::BorrowError(component_id))?)
            .ok_or(EcsError::ComponentTypeMismatch(component_id))
    }

    pub fn get_by_id<T: Component + Clone>(
        &self,
        component_id: ComponentId,
    ) -> Result<T, EcsError> {
        self.borrow_by_id(component_id).map(|c: Ref<T>| c.clone())
    }

    pub fn borrow_mut_by_id<T: Component>(
        &self,
        component_id: ComponentId,
    ) -> Result<RefMut<T>, EcsError> {
        let component = self.get_refcell(component_id)?;

        RefMut::new(component.try_borrow_mut().map_err(|_| EcsError::BorrowError(component_id))?)
            .ok_or(EcsError::ComponentTypeMismatch(component_id))
    }

    /// Collect all entity IDs into a vector.
    pub fn collect(&self, dest: &mut Vec<EntityId>) {
        dest.clear();
        dest.extend(self.entities.keys().cloned());
    }

    /// Iterator over all components of a specific type.
    pub fn components<'a, T: Component>(&'a self) -> impl Iterator<Item=ComponentId> + 'a {
        self.components.iter()
            // This filters out everything with the wrong type.
            .filter(|(_, entry)| entry.type_id == TypeId::of::<T>())
            
            // This gives an iterator over component ids.
            .map(|(&id, _)| id)
    }

    /// Iterator over all components of a specific type, yielding a reference to each.
    /// Note that this will panic when advancing the iterator if the next component is
    /// currently mutably borrowed.
    pub fn components_ref<'a, T: Component>(&'a self) -> impl Iterator<Item=Ref<'a, T>> {
        Iter::new(self.components::<T>(), self)
    }

    
    /// Iterator over all components of a specific type, yielding a mutable reference
    /// to each. Note that this will panic when advancing the iterator if the next
    /// component is currently borrowed.
    pub fn components_mut<T: Component>(&self) -> impl Iterator<Item=RefMut<T>> {
        IterMut::new(self.components::<T>(), self)
    }
}

pub struct Iter<'a, I: Iterator<Item=ComponentId>, T: Component> {
    iter: I,
    parent: &'a Ecs,
    p: PhantomData<T>
}

impl<'a, I: Iterator<Item=ComponentId>, T: Component> Iter<'a, I, T> {
    fn new(iter: I, parent: &'a Ecs) -> Self {
        Iter {
            iter,
            parent,
            p: PhantomData
        }
    }
}

impl<'a, I: Iterator<Item=ComponentId>, T: Component> Iterator for Iter<'a, I, T> {
    type Item = Ref<'a, T>;

    fn next(&mut self) -> Option<Ref<'a, T>> {
        Some(self.parent.borrow_by_id(self.iter.next()?).unwrap())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

pub struct IterMut<'a, I: Iterator<Item=ComponentId>, T: Component> {
    iter: I,
    parent: &'a Ecs,
    p: PhantomData<T>
}

impl<'a, I: Iterator<Item=ComponentId>, T: Component> IterMut<'a, I, T> {
    fn new(iter: I, parent: &'a Ecs) -> Self {
        IterMut {
            iter,
            parent,
            p: PhantomData
        }
    }
}

impl<'a, I: Iterator<Item=ComponentId>, T: Component> Iterator for IterMut<'a, I, T> {
    type Item = RefMut<'a, T>;

    fn next(&mut self) -> Option<RefMut<'a, T>> {
        Some(self.parent.borrow_mut_by_id(self.iter.next()?).unwrap())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

pub struct Ref<'a, T: 'static> {
    data: cell::Ref<'a, Box<Any>>,
    p: PhantomData<T>
}

impl<'a, T: 'static> Ref<'a, T> {
    pub fn new(data: cell::Ref<'a, Box<Any>>) -> Option<Self> {
        if (*data).downcast_ref::<T>().is_some() {
            Some(Ref {
                data,
                p: PhantomData
            })
        } else {
            None
        }
    }
}

impl<'a, T: 'static> Deref for Ref<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // This won't panic, because we ensured downcast_ref worked in Ref::new
        (*self.data).downcast_ref().unwrap()
    }
}

pub struct RefMut<'a, T: 'static> {
    data: cell::RefMut<'a, Box<Any>>,
    p: PhantomData<T>
}

impl<'a, T: 'static> RefMut<'a, T> {
    pub fn new(data: cell::RefMut<'a, Box<Any>>) -> Option<Self> {
        if (*data).downcast_ref::<T>().is_some() {
            Some(RefMut {
                data,
                p: PhantomData
            })
        } else {
            None
        }
    }
}

impl<'a, T: 'static> Deref for RefMut<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // This won't panic, because we ensured downcast_ref worked in RefMut::new
        (*self.data).downcast_ref().unwrap()
    }
}

impl<'a, T: 'static> DerefMut for RefMut<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        // This won't panic, because we ensured downcast_ref worked in Ref::new
        (*self.data).downcast_mut().unwrap()
    }
}

#[cfg(test)]
#[allow(unused)]
mod test {
    use super::*;
    use ggez::graphics::Vector2;

    #[derive(Copy, Clone, PartialEq, Debug)]
    struct Position(Vector2);

    #[derive(Copy, Clone, PartialEq, Debug)]
    struct Velocity(Vector2);

    fn update_position(pos: &Position, vel: &Velocity) -> Position {
        Position(pos.0 + vel.0)
    }

    #[test]
    fn test_update() {
        let a_start = Vector2::new(1., 3.);
        let b_start = Vector2::new(-3., 4.);
        let c_start = Vector2::new(-0., 1.3);
        let a_vel = Vector2::new(0., 2.);
        let b_vel = Vector2::new(1., 9.);
        let mut ecs = Ecs::new();
        let a = ecs.create_entity();
        let b = ecs.create_entity();
        let c = ecs.create_entity();
        let _ = ecs.set(a, Position(a_start));
        let _ = ecs.set(a, Velocity(a_vel));
        let _ = ecs.set(b, Position(b_start));
        let _ = ecs.set(b, Velocity(b_vel));
        let _ = ecs.set(c, Position(c_start));
        let mut ids = Vec::new();
        ecs.collect(&mut ids);
        for id in ids {
            let p = ecs.get::<Position>(id);
            let v = ecs.get::<Velocity>(id);
            if let (Ok(pos), Ok(vel)) = (p, v) {
                let _ = ecs.set(id, update_position(&pos, &vel));
            }
        }
        assert!(ecs.get::<Position>(a) == Ok(Position(a_start + a_vel)));
        assert!(ecs.get::<Position>(b) == Ok(Position(b_start + b_vel)));
        assert!(ecs.get::<Position>(c) == Ok(Position(c_start)));
    }

    #[test]
    fn test_set_wont_change_id() {
        let mut ecs = Ecs::new();
        let a = ecs.create_entity();
        let id = ecs.set(a, Position(Vector2::new(0.0, 0.0))).unwrap();
        let new_id = ecs.set(a, Position(Vector2::new(1.0, 1.0))).unwrap();

        assert!(id == new_id, "Ecs::set changed the id of a component!");

        let new_value: Position = ecs.get(a).unwrap();
        assert!(new_value == Position(Vector2::new(1.0, 1.0)), "Ecs::set didn't update the component.");
    }

    #[test]
    fn test_double_mutable_borrow_fails() {
        let mut ecs = Ecs::new();
        let a = ecs.create_entity();
        let id = ecs.set(a, Position(Vector2::new(0.0, 0.0))).unwrap();
        let borrow_1 = ecs.borrow::<Position>(a).unwrap();
        let maybe_borrow_2 = ecs.borrow_mut::<Position>(a);

        assert!(maybe_borrow_2.is_err());
        println!("{:?}", *borrow_1);
    }

    #[test]
    fn test_cannot_replace_while_borrowed() {
        let mut ecs = Ecs::new();
        let a = ecs.create_entity();
        let id = ecs.set(a, Position(Vector2::new(0.0, 0.0))).unwrap();
        let borrow = ecs.borrow::<Position>(a).unwrap();
        let error = ecs.replace(a, Position(Vector2::new(1.0, 1.0)));
        
        assert!(error.is_err());
        println!("{:?}", *borrow);
    }
}
