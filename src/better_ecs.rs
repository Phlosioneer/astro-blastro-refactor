// Code heavily based on (but not copied from) recs:
// https://github.com/AndyBarron/rustic-ecs

use std::any::{Any, TypeId};
use std::collections::HashMap;

type IdNumber = u64;
type ComponentMap = HashMap<TypeId, ComponentId>;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct EntityId(IdNumber);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ComponentId(IdNumber);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum NotFound {
    Entity(EntityId),
    ComponentById(ComponentId),
    ComponentByType(TypeId),
    ComponentTypeMismatch(ComponentId),
}

pub struct Ecs {
    next_entity_id: IdNumber,
    next_component_id: IdNumber,
    entities: HashMap<EntityId, ComponentMap>,
    components: HashMap<ComponentId, (Box<Any>, EntityId)>,
}

pub trait Component: 'static {
    //fn check_requirements(&self, entity: &BEntity) -> bool;
    //fn supports_scripting(&self) -> bool;
}

impl<T: Any + 'static> Component for T {}

impl Ecs {
    pub fn new() -> Self {
        Ecs {
            next_entity_id: 0,
            next_component_id: 0,
            entities: HashMap::new(),
            components: HashMap::new(),
        }
    }

    pub fn create_entity(&mut self) -> EntityId {
        let new_id_number = self.next_entity_id;
        self.next_entity_id = self.next_entity_id.checked_add(1).unwrap();
        let new_id = EntityId(new_id_number);

        self.entities.insert(new_id, HashMap::new());

        new_id
    }

    // Note: Does not touch the entities map.
    fn create_component<T: Component>(&mut self, component: T, parent: EntityId) -> ComponentId {
        let new_id_number = self.next_component_id;
        self.next_component_id = self.next_component_id.checked_add(1).unwrap();
        let new_id = ComponentId(new_id_number);

        self.components
            .insert(new_id, (Box::new(component), parent));

        new_id
    }

    // Note: Does not touch the entities map.
    // Inverse of create_component.
    pub fn remove_entity(&mut self, entity: EntityId) -> Result<(), NotFound> {
        let components = match self.entities.remove(&entity) {
            Some(components) => components,
            None => return Err(NotFound::Entity(entity)),
        };

        for (_, id) in components {
            // TODO: This is an internal error; make an error message for it.
            self.components.remove(&id).unwrap();
        }

        Ok(())
    }

    fn remove_component(&mut self, component: ComponentId) -> Result<(), NotFound> {
        match self.components.remove(&component) {
            Some(_) => Ok(()),
            None => Err(NotFound::ComponentById(component)),
        }
    }

    pub fn set<T: Component>(
        &mut self,
        entity: EntityId,
        component: T,
    ) -> Result<ComponentId, NotFound> {
        let component_id = self.create_component(component, entity);
        let entity_components = match self.entities.get_mut(&entity) {
            Some(v) => v,
            None => {
                self.remove_component(component_id).unwrap();
                return Err(NotFound::Entity(entity));
            }
        };

        entity_components.insert(TypeId::of::<T>(), component_id);

        Ok(component_id)
    }

    pub fn borrow<T: Component>(&self, entity: EntityId) -> Result<&T, NotFound> {
        let entity_components = match self.entities.get(&entity) {
            Some(v) => v,
            None => return Err(NotFound::Entity(entity)),
        };

        let component_id = match entity_components.get(&TypeId::of::<T>()) {
            Some(v) => v,
            None => return Err(NotFound::ComponentByType(TypeId::of::<T>())),
        };

        let (component, _) = self.components.get(&component_id).unwrap();

        Ok(component.downcast_ref().unwrap())
    }

    pub fn get<T: Component + Clone>(&self, entity: EntityId) -> Result<T, NotFound> {
        self.borrow(entity).map(|c: &T| c.clone())
    }

    pub fn borrow_mut<T: Component>(&mut self, entity: EntityId) -> Result<&mut T, NotFound> {
        let entity_components = match self.entities.get(&entity) {
            Some(v) => v,
            None => return Err(NotFound::Entity(entity)),
        };

        let component_id = match entity_components.get(&TypeId::of::<T>()) {
            Some(v) => v,
            None => return Err(NotFound::ComponentByType(TypeId::of::<T>())),
        };

        let (component, _) = self.components.get_mut(&component_id).unwrap();

        Ok(component.downcast_mut().unwrap())
    }

    pub fn borrow_by_id<T: Component>(&self, component_id: ComponentId) -> Result<&T, NotFound> {
        let component = match self.components.get(&component_id) {
            Some(pair) => &pair.0,
            None => return Err(NotFound::ComponentById(component_id)),
        };

        match component.downcast_ref() {
            Some(&c) => Ok(c),
            None => Err(NotFound::ComponentTypeMismatch(component_id)),
        }
    }

    pub fn get_by_id<T: Component + Clone>(
        &self,
        component_id: ComponentId,
    ) -> Result<T, NotFound> {
        self.borrow_by_id(component_id).map(|c: &T| c.clone())
    }

    pub fn borrow_mut_by_id<T: Component>(
        &mut self,
        component_id: ComponentId,
    ) -> Result<&mut T, NotFound> {
        let component = match self.components.get_mut(&component_id) {
            Some(pair) => &mut pair.0,
            None => return Err(NotFound::ComponentById(component_id)),
        };

        component
            .downcast_mut()
            .ok_or(NotFound::ComponentTypeMismatch(component_id))
    }

    /// Collect all entity IDs into a vector.
    pub fn collect(&self, dest: &mut Vec<EntityId>) {
        dest.clear();
        dest.extend(self.entities.keys().cloned());
    }
}

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
}
