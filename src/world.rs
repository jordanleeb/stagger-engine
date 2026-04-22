use crate::component::{ComponentId, ComponentRegistry};
use crate::entity::{Entity, EntityAllocator};

/// Owns the core ECS state.
///
/// For now, the world is responsible for:
/// - Allocating and destroying entities.
/// - Tracking entity liveness.
/// - Registering component types.
///
/// Later, it will also own component storage and archetypes.
pub struct World {
    entities: EntityAllocator,
    components: ComponentRegistry,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// Creates a new, empty world.
    pub fn new() -> Self {
        Self {
            entities: EntityAllocator::new(),
            components: ComponentRegistry::new(),
        }
    }

    // Spawns a new entity.
    pub fn spawn(&mut self) -> Entity {
        self.entities.create()
    }

    /// Destroys an entity.
    pub fn destroy(&mut self, entity: Entity) -> bool {
        self.entities.destroy(entity)
    }

    /// Returns `true` if the entity is currently alive.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.entities.is_alive(entity)
    }

    /// Registers a component type and returns its ID.
    pub fn register_component<T: 'static>(&mut self) -> ComponentId {
        self.components.register::<T>()
    }

    /// Returns the ID of a registered component type.
    pub fn component_id<T: 'static>(&self) -> Option<ComponentId> {
        self.components.get::<T>()
    }

    /// Returns `true` if the component type has been registered.
    pub fn has_component_type<T: 'static>(&self) -> bool {
        self.components.contains::<T>()
    }

    /// Clears the world.
    ///
    /// This removes all entities and resets all component type registrations,
    /// returning the world to a fresh empty state.
    pub fn clear(&mut self) {
        self.entities.clear();
        self.components = ComponentRegistry::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Position;

    #[test]
    fn spawn_creates_alive_entity() {
        let mut world = World::new();
        let e = world.spawn();
        assert!(world.is_alive(e));
    }

    #[test]
    fn destroy_marks_entity_as_dead() {
        let mut world = World::new();
        let e = world.spawn();

        assert!(world.destroy(e));
        assert!(!world.is_alive(e));
    }

    #[test]
    fn register_component_through_world() {
        let mut world = World::new();

        let id = world.register_component::<Position>();

        assert_eq!(world.component_id::<Position>(), Some(id));
        assert!(world.has_component_type::<Position>());
    }

    #[test]
    fn clear_resets_world() {
        let mut world = World::new();

        let e = world.spawn();
        world.register_component::<Position>();

        world.clear();

        assert!(!world.is_alive(e));
        assert_eq!(world.component_id::<Position>(), None);
        assert!(!world.has_component_type::<Position>());
    }

    #[test]
    fn destorying_twice_fails() {
        let mut world = World::new();
        let e = world.spawn();

        assert!(world.destroy(e));
        assert!(!world.destroy(e));
    }
}
