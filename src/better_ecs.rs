// Code heavily based on (but not copied from) recs:
// https://github.com/AndyBarron/rustic-ecs

///! This library is heavily based on Rustic Ecs ("Recs"), go there if
///! documentation here is lacking: https://github.com/AndyBarron/rustic-ecs
use std::any::{Any, TypeId};
use std::cell::{self, RefCell};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::Mutex;

use super::util::RefCellTryReplaceExt;

lazy_static! {
    static ref NEXT_ECS_ID: Mutex<IdNumber> = Mutex::new(0);
}

type IdNumber = u64;
type ComponentMap = HashMap<TypeId, ComponentId>;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct EcsId(IdNumber);

impl EcsId {
    pub fn new() -> Self {
        let mut next_id_lock = NEXT_ECS_ID.lock().unwrap();
        let id = *next_id_lock;
        *next_id_lock = next_id_lock.wrapping_add(1);

        EcsId(id)
    }
}

/// A unique ID tag for an entity in an Ecs system.
///
/// There is a static guarantee that no two entities in the same Ecs will
/// ever share an EntityId, including deleted entities.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct EntityId(EcsId, IdNumber);

/// A unique ID tag for a component in an Ecs system.
///
/// There is a static guarantee that no two components in the same Ecs will
/// ever share a ComponentId, including deleted or replaced components.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ComponentId(EcsId, IdNumber);

/// A convenient way to store ComponentIds with type information.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ComponentRef<T: Component> {
    id: ComponentId,
    p: PhantomData<T>,
}

impl<T: Component> ComponentRef<T> {
    pub fn new(id: ComponentId) -> Self {
        ComponentRef { id, p: PhantomData }
    }

    pub fn from_entity(id: EntityId, ecs: &Ecs) -> Result<Self, EcsError> {
        Ok(Self::new(ecs.lookup_component::<T>(id)?))
    }

    pub fn borrow<'a>(&self, ecs: &'a Ecs) -> Result<Ref<'a, T>, EcsError> {
        ecs.borrow_by_id(self.id)
    }

    pub fn borrow_mut<'a>(&self, ecs: &'a Ecs) -> Result<RefMut<'a, T>, EcsError> {
        ecs.borrow_mut_by_id(self.id)
    }
}

impl<T: Component + Clone> ComponentRef<T> {
    pub fn get(&self, ecs: &Ecs) -> Result<T, EcsError> {
        ecs.get_by_id(self.id)
    }
}

impl<T: Component> From<ComponentId> for ComponentRef<T> {
    fn from(other: ComponentId) -> ComponentRef<T> {
        ComponentRef::new(other)
    }
}

/// An error type for the Ecs.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EcsError {
    /// The requested entity doesn't exist.
    EntityNotFound(EntityId),

    /// The requested component doesn't exist.
    ComponentNotFound(ComponentId),

    /// The given entity doesn't have a component of the requested type.
    ComponentTypeNotFound(EntityId),

    /// The requested component doesn't have the expected type.
    ComponentTypeMismatch(ComponentId),

    /// The requested component cannot be borrowed right now.
    BorrowError(ComponentId),

    /// Some internal error occurred; this indicates that there is a bug
    /// in the library.
    InternalError(&'static str, Option<Box<EcsError>>),
}

struct ComponentEntry {
    pub refbox: RefCell<Box<Any>>,
    pub parent: EntityId,
    pub type_id: TypeId,
}

impl ComponentEntry {
    pub fn new<T: Component>(component: T, parent: EntityId) -> Self {
        ComponentEntry {
            refbox: RefCell::new(Box::new(component)),
            parent,
            type_id: TypeId::of::<T>(),
        }
    }
}

/// The Entity Component System object. It contains all entities and
/// components.
pub struct Ecs {
    // The id for this ECS.
    ecs_id: EcsId,

    /// The next free EntityId number.
    next_entity_id: IdNumber,

    /// The next free ComponentId number.
    next_component_id: IdNumber,

    /// A map of entity Ids to their components.
    entities: HashMap<EntityId, ComponentMap>,

    /// A map of component Ids to component data.
    components: HashMap<ComponentId, ComponentEntry>,
}

/// This is a trait for all components. It's auto-implemented for everything.
pub trait Component: 'static {}

impl<T: 'static> Component for T {}

impl Ecs {
    /// Create a new Ecs.
    pub fn new() -> Self {
        Ecs {
            ecs_id: EcsId::new(),
            next_entity_id: 0,
            next_component_id: 0,
            entities: HashMap::new(),
            components: HashMap::new(),
        }
    }

    /// Create a new Ecs with no allocations. Useful when paired with the
    /// Ecs::merge function.
    pub fn empty() -> Self {
        Ecs {
            ecs_id: EcsId::new(),
            next_entity_id: 0,
            next_component_id: 0,
            entities: HashMap::with_capacity(0),
            components: HashMap::with_capacity(0),
        }
    }

    /// Merge two Ecs instances together.
    ///
    /// This is particularly useful when a component being called through
    /// Ecs::components_ref or Ecs::components_mut needs to add a new entity
    /// to the system. Instead, you can create a new, empty Ecs, pass it by
    /// mutable reference, then merge it with the old one outside of the iterator.
    ///
    /// Merging an Ecs that is empty is free.
    pub fn merge(&mut self, other: Ecs) {
        if other.entities.len() == 0 {
            return;
        }

        self.components.extend(other.components);
        self.entities.extend(other.entities);
    }

    fn create_entity_id(&mut self) -> Option<EntityId> {
        let new_id_number = self.next_entity_id;
        self.next_entity_id = self.next_entity_id.checked_add(1)?;
        Some(EntityId(self.ecs_id, new_id_number))
    }

    fn create_component_id(&mut self) -> Option<ComponentId> {
        let new_id_number = self.next_component_id;
        self.next_component_id = self.next_component_id.checked_add(1)?;
        Some(ComponentId(self.ecs_id, new_id_number))
    }

    /// Create an entity, or return `None` if no more `EntityIds` can be
    /// created.
    ///
    /// The ECS can go through `n^64-1` unique ID's before panicking,
    /// so this practically never fails.
    ///
    /// This is the non-panicking variant of `create_entity`.
    pub fn try_create_entity(&mut self) -> Option<EntityId> {
        let new_id = self.create_entity_id()?;

        self.entities.insert(new_id, HashMap::new());

        Some(new_id)
    }

    /// Create an entity.
    ///
    /// # Panics
    ///
    /// Panics if no more unique `EntityIds` can be generated.
    ///
    /// The ECS can go through `n^64-1` unique ID's before panicking,
    /// so this practically never fails.
    pub fn create_entity(&mut self) -> EntityId {
        self.try_create_entity().unwrap()
    }

    // Note: Does not touch the entities map.
    fn create_component<T: Component>(&mut self, component: T, parent: EntityId) -> ComponentId {
        // TODO: Unwrap
        let new_id = self.create_component_id().unwrap();

        self.components
            .insert(new_id, ComponentEntry::new(component, parent));

        new_id
    }

    /// Delete an entity and all components attached to it. Returns an error
    /// if `entity` doesn't exist.
    pub fn remove_entity(&mut self, entity: EntityId) -> Result<(), EcsError> {
        let components = match self.entities.remove(&entity) {
            Some(components) => components,
            None => return Err(EcsError::EntityNotFound(entity)),
        };

        // Remove all the components attached to the entity.
        for (_, id) in components {
            self.components.remove(&id).ok_or(EcsError::InternalError(
                "Failed to remove component attached to an entity.",
                None,
            ))?;
        }

        Ok(())
    }

    // Note: Does not touch the entities map.
    // Inverse of create_component.
    fn remove_component(&mut self, component: ComponentId) -> Result<(), EcsError> {
        match self.components.remove(&component) {
            Some(_) => Ok(()),
            None => Err(EcsError::ComponentNotFound(component)),
        }
    }

    /// Returns true if `entity` exists; false otherwise.
    pub fn has_entity(&self, entity: EntityId) -> bool {
        self.entities.contains_key(&entity)
    }

    /// Returns true if `component` exists; false otherwise.
    pub fn has_component_by_id(&self, component: ComponentId) -> bool {
        self.components.contains_key(&component)
    }

    /// Checks if `entity` has a component of the specified type attached
    /// to it. If it does, it returns the component's ID; None otherwise.
    ///
    /// Returns an error if `entity` doesn't exist.
    ///
    /// See also `Ecs::lookup_component`.
    pub fn has_component<T: Component>(
        &self,
        entity: EntityId,
    ) -> Result<Option<ComponentId>, EcsError> {
        let components = self
            .entities
            .get(&entity)
            .ok_or(EcsError::EntityNotFound(entity))?;
        Ok(components.get(&TypeId::of::<T>()).map(|&id| id))
    }

    /// Returns the ID of the component of type `T` on `entity`. If `entity`
    /// doesn't have a matching component, an error is returned.
    ///
    /// See also `Ecs::has_component`.
    pub fn lookup_component<T: Component>(
        &self,
        entity: EntityId,
    ) -> Result<ComponentId, EcsError> {
        self.has_component::<T>(entity)
            .and_then(|opt| opt.ok_or(EcsError::ComponentTypeNotFound(entity)))
    }

    /// Returns the ID of the entity that `component` is attached to.
    pub fn get_parent(&self, component: ComponentId) -> Result<EntityId, EcsError> {
        self.components
            .get(&component)
            .ok_or(EcsError::ComponentNotFound(component))
            .map(|data| data.parent)
    }

    /// Returns true if `component` is attached to `entity`. Returns an error if
    /// the component or the entity don't exist.
    pub fn is_component_attached(
        &self,
        entity: EntityId,
        component: ComponentId,
    ) -> Result<bool, EcsError> {
        let components = self
            .entities
            .get(&entity)
            .ok_or(EcsError::EntityNotFound(entity))?;
        Ok(components.iter().filter(|(_, &id)| id == component).count() > 0)
    }

    /// Returns true if `component` is the specified type; false otherwise. Returns
    /// an error if the component doesn't exist.
    pub fn component_is_type<T: Component>(
        &self,
        component: ComponentId,
    ) -> Result<bool, EcsError> {
        let component_data = self
            .components
            .get(&component)
            .ok_or(EcsError::ComponentNotFound(component))?;
        Ok(component_data.type_id == TypeId::of::<T>())
    }

    // Note: This will force the new component to have a different EntityId than the old one.
    fn create_and_attach_component<T: Component>(
        &mut self,
        entity: EntityId,
        component: T,
    ) -> Result<ComponentId, EcsError> {
        let component_id = self.create_component(component, entity);
        let entity_components = self
            .entities
            .get_mut(&entity)
            .ok_or(EcsError::EntityNotFound(entity))?;

        let maybe_old_id = entity_components.insert(TypeId::of::<T>(), component_id);
        if let Some(old_id) = maybe_old_id {
            self.remove_component(old_id).map_err(|e| {
                EcsError::InternalError("Failed to remove old component.", Some(Box::new(e)))
            })?;
        }

        Ok(component_id)
    }

    /// Set the component on `entity` for type `T` to `component`. Unlike `Ecs::set`, this will
    /// return an error rather than create a new component.
    ///
    /// If successful, the replaced component is returned.
    pub fn replace<T: Component>(&self, entity: EntityId, component: T) -> Result<T, EcsError> {
        if let Some(component_id) = self.has_component::<T>(entity)? {
            self.replace_by_id_unchecked(component_id, component)
        } else {
            Err(EcsError::ComponentTypeNotFound(entity))
        }
    }

    /// Replace the component at `component_id` with `component`. Returns an error if the component doesn't
    /// exist, or if the type `T` is incorrect.
    ///
    /// If successful, the replaced component is returned.
    pub fn replace_by_id<T: Component>(
        &self,
        component_id: ComponentId,
        component: T,
    ) -> Result<T, EcsError> {
        if self.component_is_type::<T>(component_id)? {
            self.replace_by_id_unchecked(component_id, component)
        } else {
            Err(EcsError::ComponentTypeMismatch(component_id))
        }
    }

    fn replace_by_id_unchecked<T: Component>(
        &self,
        component_id: ComponentId,
        component: T,
    ) -> Result<T, EcsError> {
        let boxed_any = self
            .get_refcell(component_id)?
            .try_replace(Box::new(component))
            .map_err(|_| EcsError::BorrowError(component_id))?;

        boxed_any
            .downcast::<T>()
            .map(|boxed_t| *boxed_t)
            .map_err(|_| {
                panic!("Typecheck succeded and then failed! Ecs left in inconsistent state!")
            })
    }

    /// Set the component on `entity` for type `T` to `component`. If `entity` doesn't
    /// already have a component of type `T`, this creates a new one.
    ///
    /// If successful, the new component's `ComponentId` is returned.
    pub fn set<T: Component>(
        &mut self,
        entity: EntityId,
        component: T,
    ) -> Result<ComponentId, EcsError> {
        match self.has_component::<T>(entity)? {
            Some(component_id) => {
                // Update that component.
                self.replace::<T>(entity, component).map(|_| component_id)
            }
            None => self
                .create_and_attach_component(entity, component)
                .map_err(|e| {
                    if let EcsError::EntityNotFound(_) = e {
                        EcsError::InternalError(
                            "Couldn't find entity the 2nd time after finding it the 1st.",
                            Some(Box::new(e)),
                        )
                    } else {
                        e
                    }
                }),
        }
    }

    fn get_refcell(&self, id: ComponentId) -> Result<&RefCell<Box<Any>>, EcsError> {
        self.components
            .get(&id)
            .map(|v| &v.refbox)
            .ok_or(EcsError::ComponentNotFound(id))
    }

    /// Get a copy of the specified component.
    ///
    /// Returns an error if a mutable borrow of this component already exists.
    pub fn get<T: Component + Clone>(&self, entity: EntityId) -> Result<T, EcsError> {
        self.borrow(entity).map(|c: Ref<T>| c.clone())
    }

    /// Get an immutable borrow of the specified component.
    /// Borrows of components are independent of each other.
    ///
    /// Any number of immutable borrows of a given component can exist at the
    /// same time.
    ///
    /// Returns an error if a mutable borrow of this component already exists.
    pub fn borrow<T: Component>(&self, entity: EntityId) -> Result<Ref<T>, EcsError> {
        let id = self.lookup_component::<T>(entity)?;
        let refcell = self.get_refcell(id)?;
        let refbox = refcell
            .try_borrow()
            .map_err(|_| EcsError::BorrowError(id))?;
        Ref::new(refbox).ok_or(EcsError::ComponentTypeMismatch(id))
    }

    /// Get a mutable borrow of the specified component.
    /// Borrows of components are independant of each other.
    ///
    /// Exactly one mutable borrow of a given component can exist. No immutable borrows
    /// of that component are allowed while it is mutably borrowed.
    ///
    /// Returns and error if a mutable or immutable borrow of this component already exists.
    pub fn borrow_mut<T: Component>(&self, entity: EntityId) -> Result<RefMut<T>, EcsError> {
        let id = self.lookup_component::<T>(entity)?;
        let refcell = self.get_refcell(id)?;
        let refbox = refcell
            .try_borrow_mut()
            .map_err(|_| EcsError::BorrowError(id))?;
        RefMut::new(refbox).ok_or(EcsError::ComponentTypeMismatch(id))
    }

    /// Get a copy of the specified component.
    ///
    /// Returns an error if a mutable borrow of this component already exists.
    pub fn get_by_id<T: Component + Clone>(
        &self,
        component_id: ComponentId,
    ) -> Result<T, EcsError> {
        self.borrow_by_id(component_id).map(|c: Ref<T>| c.clone())
    }

    /// Get an immutable borrow of the specified component.
    /// Borrows of components are independent of each other.
    ///
    /// Any number of immutable borrows of a given component can exist at the
    /// same time.
    ///
    /// Returns an error if a mutable borrow of this component already exists.
    pub fn borrow_by_id<T: Component>(
        &self,
        component_id: ComponentId,
    ) -> Result<Ref<T>, EcsError> {
        let component = self.get_refcell(component_id)?;

        Ref::new(
            component
                .try_borrow()
                .map_err(|_| EcsError::BorrowError(component_id))?,
        ).ok_or(EcsError::ComponentTypeMismatch(component_id))
    }

    /// Get a mutable borrow of the specified component.
    /// Borrows of components are independant of each other.
    ///
    /// Exactly one mutable borrow of a given component can exist. No immutable borrows
    /// of that component are allowed while it is mutably borrowed.
    ///
    /// Returns and error if a mutable or immutable borrow of this component already exists.
    pub fn borrow_mut_by_id<T: Component>(
        &self,
        component_id: ComponentId,
    ) -> Result<RefMut<T>, EcsError> {
        let component = self.get_refcell(component_id)?;

        RefMut::new(
            component
                .try_borrow_mut()
                .map_err(|_| EcsError::BorrowError(component_id))?,
        ).ok_or(EcsError::ComponentTypeMismatch(component_id))
    }

    /// Collect all entity IDs into a vector.
    pub fn collect(&self, dest: &mut Vec<EntityId>) {
        dest.clear();
        dest.extend(self.entities.keys().cloned());
    }

    /// Iterator over all components of a specific type.
    pub fn components<'a, T: Component>(&'a self) -> impl Iterator<Item = ComponentId> + 'a {
        self.components
            .iter()
            // This filters out everything with the wrong type.
            .filter(|(_, entry)| entry.type_id == TypeId::of::<T>())
            // This gives an iterator over component ids.
            .map(|(&id, _)| id)
    }

    /// Iterator over all components of a specific type, yielding a reference to each.
    /// Note that this will panic when advancing the iterator if the next component is
    /// currently mutably borrowed.
    pub fn components_ref<T: Component>(&self) -> impl Iterator<Item = (ComponentId, Ref<T>)> {
        Iter::new(self.components::<T>(), self)
    }

    /// Iterator over all components of a specific type, yielding a mutable reference
    /// to each. Note that this will panic when advancing the iterator if the next
    /// component is currently borrowed.
    pub fn components_mut<T: Component>(&self) -> impl Iterator<Item = (ComponentId, RefMut<T>)> {
        IterMut::new(self.components::<T>(), self)
    }

    pub fn entities_with<T: Component>(&self) -> Vec<EntityId> {
        self.components::<T>()
            .map(|id| self.get_parent(id).unwrap())
            .collect()
    }
}

pub struct Iter<'a, I: Iterator<Item = ComponentId>, T: Component> {
    iter: I,
    parent: &'a Ecs,
    p: PhantomData<T>,
}

impl<'a, I: Iterator<Item = ComponentId>, T: Component> Iter<'a, I, T> {
    fn new(iter: I, parent: &'a Ecs) -> Self {
        Iter {
            iter,
            parent,
            p: PhantomData,
        }
    }
}

impl<'a, I: Iterator<Item = ComponentId>, T: Component> Iterator for Iter<'a, I, T> {
    type Item = (ComponentId, Ref<'a, T>);

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.iter.next()?;
        Some((id, self.parent.borrow_by_id(id).unwrap()))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

pub struct IterMut<'a, I: Iterator<Item = ComponentId>, T: Component> {
    iter: I,
    parent: &'a Ecs,
    p: PhantomData<T>,
}

impl<'a, I: Iterator<Item = ComponentId>, T: Component> IterMut<'a, I, T> {
    fn new(iter: I, parent: &'a Ecs) -> Self {
        IterMut {
            iter,
            parent,
            p: PhantomData,
        }
    }
}

impl<'a, I: Iterator<Item = ComponentId>, T: Component> Iterator for IterMut<'a, I, T> {
    type Item = (ComponentId, RefMut<'a, T>);

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.iter.next()?;
        Some((id, self.parent.borrow_mut_by_id(id).unwrap()))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

pub struct Ref<'a, T: 'static> {
    data: cell::Ref<'a, Box<Any>>,
    p: PhantomData<T>,
}

impl<'a, T: 'static> Ref<'a, T> {
    pub fn new(data: cell::Ref<'a, Box<Any>>) -> Option<Self> {
        if (*data).downcast_ref::<T>().is_some() {
            Some(Ref {
                data,
                p: PhantomData,
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
    p: PhantomData<T>,
}

impl<'a, T: 'static> RefMut<'a, T> {
    pub fn new(data: cell::RefMut<'a, Box<Any>>) -> Option<Self> {
        if (*data).downcast_ref::<T>().is_some() {
            Some(RefMut {
                data,
                p: PhantomData,
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
        assert!(
            new_value == Position(Vector2::new(1.0, 1.0)),
            "Ecs::set didn't update the component."
        );
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
