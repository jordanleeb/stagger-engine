use crate::component::{ComponentId, ComponentRegistry};
use crate::entity::{Entity, EntityAllocator};
use crate::location::EntityLocation;
use crate::archetype::{Archetype, ArchetypeId, ArchetypeSignature};

/// Owns the core ECS state.
///
/// The world is responsible for:
/// - Allocating and destroying entities.
/// - Tracking entity liveness.
/// - Registering component types.
/// - Owning all archetypes and their entity storage.
/// - Tracking the location of each entity within archetypes.
/// 
/// Entities are stored in archetypes based on their component sets.
/// Each entity has an associated [`EntityLocation`] that identifies
/// which archetype it belongs to and which row it occupies.
/// 
/// # Invariants
/// 
/// - Every alive entity exists in exactly one archetype.
/// - `entity_locations[index]` matches the entity's actual position.
/// - Archetype storage is dense; removals use swap-remove.
/// - When swap-remove moves an entity, its location is updated.
/// 
/// # Notes
/// 
/// Component data is not yet stored in archetypes. This will be added
/// in a later phase, along with support for moving entities between
/// archetypes when components are added or removed.
pub struct World {
    entities: EntityAllocator,
    components: ComponentRegistry,
    entity_locations: Vec<Option<EntityLocation>>,
    archetypes: Vec<Archetype>,
    empty_archetype: ArchetypeId,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// Creates a new, empty world.
    pub fn new() -> Self {
        let empty_archetype = Archetype::new(ArchetypeSignature::new(vec![]));

        Self {
            entities: EntityAllocator::new(),
            components: ComponentRegistry::new(),
            entity_locations: Vec::new(),
            archetypes: vec![empty_archetype],
            empty_archetype: 0,
        }
    }

    /// Spawns a new entity and places it into the empty archetype.
    pub fn spawn(&mut self) -> Entity {
        let entity = self.entities.create();
        self.ensure_entity_slot(entity);

        let empty_id = self.empty_archetype;
        let row = self.archetypes[empty_id as usize].push_entity(entity);

        let location = EntityLocation::new(empty_id, row);
        self.entity_locations[entity.index as usize] = Some(location);

        entity
    }

    /// Destroys an entity.
    /// 
    /// Removes it from its archetype, updates any entity moved by
    /// swap-remove, clears its location, and invalidates the handle.
    pub fn destroy(&mut self, entity: Entity) -> bool {
        if !self.is_alive(entity) {
            return false;
        }

        let location = match self.location(entity) {
            Some(location) => location,
            None => return false,
        };

        let archetype_id = location.archetype();
        let row = location.row();

        let archetype = &mut self.archetypes[archetype_id as usize];
        let removed = archetype.remove_entity_row(row);

        debug_assert_eq!(removed, entity);

        // Handle swap-remove: update moved entity
        if row < archetype.len() {
            let moved_entity = archetype.entities()[row];
            let moved_index = moved_entity.index as usize;
            self.entity_locations[moved_index] = Some(EntityLocation::new(archetype_id, row));
        }

        // Clear location
        let index = entity.index as usize;
        self.entity_locations[index] = None;

        // Finally destroy in allocator
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
        self.entity_locations.clear();
        self.archetypes.clear();
        self.archetypes.push(Archetype::new(ArchetypeSignature::new(vec![])));
        self.empty_archetype = 0;
    }

    fn ensure_entity_slot(&mut self, entity: Entity) {
        let index = entity.index as usize;

        if index >= self.entity_locations.len() {
            self.entity_locations.resize(index + 1, None);
        }
    }

    /// Returns the stored location of an entity, if any.
    pub fn location(&self, entity: Entity) -> Option<EntityLocation> {
        if !self.is_alive(entity) {
            return None;
        }

        let index = entity.index as usize;
        self.entity_locations.get(index).copied().flatten()
    }

    /// Sets the location of an entity.
    /// 
    /// Returns `false` if the entity is not alive.
    pub fn set_location(&mut self, entity: Entity, location: EntityLocation) -> bool {
        if !self.is_alive(entity) {
            return false;
        }

        self.ensure_entity_slot(entity);

        let index = entity.index as usize;
        self.entity_locations[index] = Some(location);
        true
    }

    /// Clears the stored location of an entity.
    /// 
    /// Returns `false` if the entity is not alive.
    pub fn clear_location(&mut self, entity: Entity) -> bool {
        if !self.is_alive(entity) {
            return false;
        }

        let index = entity.index as usize;

        if let Some(slot) = self.entity_locations.get_mut(index) {
            *slot = None;
            return true;
        }

        false
    }

    /// Returns the number of archetypes in the world.
    pub fn archetype_count(&self) -> usize {
        self.archetypes.len()
    }

    /// Returns the ID of the empty archetype.
    pub fn empty_archetype_id(&self) -> ArchetypeId {
        self.empty_archetype
    }

    /// Returns an archetype by ID.
    pub fn archetype(&self, id: ArchetypeId) -> Option<&Archetype> {
        self.archetypes.get(id as usize)
    }

    fn find_archetype_by_signature(&self, signature: &ArchetypeSignature) -> Option<ArchetypeId> {
        self.archetypes
            .iter()
            .position(|archetype| archetype.signature() == signature)
            .map(|index| index as ArchetypeId)
    }

    fn find_or_create_archetype(&mut self, signature: ArchetypeSignature) -> ArchetypeId {
        if let Some(id) = self.find_archetype_by_signature(&signature) {
            return id;
        }

        let id = self.archetypes.len() as ArchetypeId;
        self.archetypes.push(Archetype::new(signature));
        id
    }

    fn move_entity_to_archetype(
        &mut self,
        entity: Entity,
        destination_archetype: ArchetypeId,
    ) -> bool {
        if !self.is_alive(entity) {
            return false;
        }

        let source_location = match self.location(entity) {
            Some(location) => location,
            None => return false,
        };

        let source_archetype = source_location.archetype();
        let source_row = source_location.row();

        if source_archetype == destination_archetype {
            return true;
        }

        // Remove from source archetype
        let removed = {
            let archetype = &mut self.archetypes[source_archetype as usize];
            archetype.remove_entity_row(source_row)
        };

        debug_assert_eq!(removed, entity);

        // If swap-remove moved another entity into the old row,
        // update that entity's location.
        let source_len = self.archetypes[source_archetype as usize].len();
        if source_row < source_len {
            let moved_entity = self.archetypes[source_archetype as usize].entities()[source_row];
            let moved_index = moved_entity.index as usize;

            self.entity_locations[moved_index] = 
                Some(EntityLocation::new(source_archetype, source_row));
        }

        // Insert into destination archetype.
        let destination_row = {
            let archetype = &mut self.archetypes[destination_archetype as usize];
            archetype.push_entity(entity)
        };

        let entity_index = entity.index as usize;
        self.entity_locations[entity_index] = 
            Some(EntityLocation::new(destination_archetype, destination_row));

        true
    }

    /// Moves an entity into the archetype identified by `signature`.
    /// 
    /// Creates the destination archetype if it does not yet exist.
    pub fn move_entity_by_signature(
        &mut self,
        entity: Entity,
        signature: ArchetypeSignature,
    ) -> bool {
        let destination = self.find_or_create_archetype(signature);
        self.move_entity_to_archetype(entity, destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Position {
        x: f32,
        y: f32,
    }

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
    fn destroying_twice_fails() {
        let mut world = World::new();
        let e = world.spawn();

        assert!(world.destroy(e));
        assert!(!world.destroy(e));
    }

    #[test]
    fn spawned_entity_has_location_in_empty_archetype() {
        let mut world = World::new();
        let e = world.spawn();

        let location = world.location(e).unwrap();
        assert_eq!(location.archetype(), world.empty_archetype_id());
        assert_eq!(location.row(), 0);
    }

    #[test]
    fn can_set_and_get_entity_location() {
        let mut world = World::new();
        let e = world.spawn();

        let location = EntityLocation::new(2, 5);

        assert!(world.set_location(e, location));
        assert_eq!(world.location(e), Some(location));
    }

    #[test]
    fn clear_location_removes_stored_location() {
        let mut world = World::new();
        let e = world.spawn();

        let location = EntityLocation::new(1, 3);

        assert!(world.set_location(e, location));
        assert_eq!(world.location(e), Some(location));

        assert!(world.clear_location(e));
        assert_eq!(world.location(e), None);
    }

    #[test]
    fn destroy_clears_entity_location() {
        let mut world = World::new();
        let e = world.spawn();

        assert!(world.set_location(e, EntityLocation::new(0, 0)));
        assert!(world.destroy(e));
        assert_eq!(world.location(e), None);
    }

    #[test]
    fn cannot_set_location_for_dead_entity() {
        let mut world = World::new();
        let e = world.spawn();
        assert!(world.destroy(e));

        assert!(!world.set_location(e, EntityLocation::new(0, 0)));
        assert_eq!(world.location(e), None);
    }

    #[test]
    fn clear_resets_locations() {
        let mut world = World::new();
        let e = world.spawn();

        assert!(world.set_location(e, EntityLocation::new(4, 2)));
        world.clear();

        assert_eq!(world.location(e), None);
    }

    #[test]
    fn world_starts_with_one_empty_archetype() {
        let world = World::new();

        assert_eq!(world.archetype_count(), 1);

        let empty = world.archetype(world.empty_archetype_id()).unwrap();
        assert!(empty.signature().component_ids().is_empty());
        assert!(empty.is_empty());
    }

    #[test]
    fn spawn_places_entity_in_empty_archetype() {
        let mut world = World::new();
        let e = world.spawn();

        let location = world.location(e).unwrap();
        assert_eq!(location.archetype(), world.empty_archetype_id());
        assert_eq!(location.row(), 0);

        let empty = world.archetype(world.empty_archetype_id()).unwrap();
        assert_eq!(empty.entities(), &[e]);
    }

    #[test]
    fn multiple_spawns_fill_empty_archetype_rows() {
        let mut world = World::new();

        let e1 = world.spawn();
        let e2 = world.spawn();

        let l1 = world.location(e1).unwrap();
        let l2 = world.location(e2).unwrap();

        assert_eq!(l1.archetype(), world.empty_archetype_id());
        assert_eq!(l2.archetype(), world.empty_archetype_id());

        assert_eq!(l1.row(), 0);
        assert_eq!(l2.row(), 1);

        let empty = world.archetype(world.empty_archetype_id()).unwrap();
        assert_eq!(empty.entities(), &[e1, e2]);
    }

    #[test]
    fn destroy_removes_entity_from_empty_archetype() {
        let mut world = World::new();

        let e1 = world.spawn();
        let e2 = world.spawn();

        assert!(world.destroy(e1));
        assert!(!world.is_alive(e1));
        assert_eq!(world.location(e1), None);

        let empty = world.archetype(world.empty_archetype_id()).unwrap();
        assert_eq!(empty.len(), 1);
        assert_eq!(empty.entities(), &[e2]);

        let l2 = world.location(e2).unwrap();
        assert_eq!(l2.archetype(), world.empty_archetype_id());
        assert_eq!(l2.row(), 0);
    }

    #[test]
    fn clear_recreates_empty_archetype() {
        let mut world = World::new();

        world.spawn();
        world.clear();

        assert_eq!(world.archetype_count(), 1);

        let empty = world.archetype(world.empty_archetype_id()).unwrap();
        assert!(empty.signature().component_ids().is_empty());
        assert!(empty.is_empty());
    }

    #[test]
    fn moving_entity_creates_destination_archetype() {
        let mut world = World::new();
        let e = world.spawn();

        let signature = ArchetypeSignature::new(vec![1]);

        assert!(world.move_entity_by_signature(e, signature.clone()));
        assert_eq!(world.archetype_count(), 2);

        let location = world.location(e).unwrap();
        let archetype = world.archetype(location.archetype()).unwrap();

        assert_eq!(archetype.signature(), &signature);
        assert_eq!(archetype.entities(), &[e]);
    }

    #[test]
    fn moving_entity_updates_location() {
        let mut world = World::new();
        let e = world.spawn();

        let signature = ArchetypeSignature::new(vec![1, 2]);

        assert!(world.move_entity_by_signature(e, signature));

        let location = world.location(e).unwrap();
        assert_ne!(location.archetype(), world.empty_archetype_id());
        assert_eq!(location.row(), 0);
    }

    #[test]
    fn moving_entity_updates_swapped_source_entity_location() {
        let mut world = World::new();

        let e1 = world.spawn();
        let e2 = world.spawn();
        let e3 = world.spawn();

        let destination_signature = ArchetypeSignature::new(vec![42]);

        // Move middle entity out of the empty archetype.
        assert!(world.move_entity_by_signature(e2, destination_signature));

        let empty = world.archetype(world.empty_archetype_id()).unwrap();
        assert_eq!(empty.len(), 2);
        assert!(empty.entities().contains(&e1));
        assert!(empty.entities().contains(&e3));

        let e3_location = world.location(e3).unwrap();
        assert_eq!(e3_location.archetype(), world.empty_archetype_id());

        // Because of swap-remove, e3 should now occupy row 1.
        assert_eq!(e3_location.row(), 1);
    }

    #[test]
    fn moving_entity_to_same_archetype_is_no_op() {
        let mut world = World::new();
        let e = world.spawn();

        let before = world.location(e).unwrap();
        assert!(world.move_entity_by_signature(e, ArchetypeSignature::new(vec![])));

        let after = world.location(e).unwrap();
        assert_eq!(before, after);

        let empty = world.archetype(world.empty_archetype_id()).unwrap();
        assert_eq!(empty.entities(), &[e]);
    }

    #[test]
    fn dead_entity_cannot_be_moved() {
        let mut world = World::new();
        let e = world.spawn();

        assert!(world.destroy(e));
        assert!(!world.move_entity_by_signature(e, ArchetypeSignature::new(vec![1])));
    }
}
