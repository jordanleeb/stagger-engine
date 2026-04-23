use crate::archetype::{Archetype, ArchetypeId, ArchetypeSignature, RowMoveResult};
use crate::column::Column;
use crate::component::{ComponentId, ComponentRegistry};
use crate::entity::{Entity, EntityAllocator};
use crate::location::EntityLocation;

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
/// - Archetype entity storage is dense; removals use swap-remove.
/// - Component columns correspond to the archetype signature.
/// - After successful component insertion or removal, component column row
///   counts match `entities.len()`.
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
        let empty_signature = ArchetypeSignature::new(vec![]);
        let empty_archetype = Archetype::new(empty_signature, vec![]);

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
    /// Removes it from its archetype, drops all component data in its row,
    /// updates any entity moved by swap-remove, clears its location, and
    /// invalidates the handle.
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
        let removed = archetype.swap_remove_row_and_drop_components(row);

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
        self.archetypes
            .push(Archetype::new(ArchetypeSignature::new(vec![]), vec![]));
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

        let columns = self.build_columns_for_signature(&signature);
        let id = self.archetypes.len() as ArchetypeId;
        self.archetypes.push(Archetype::new(signature, columns));
        id
    }

    /// Transfers an entity from its current archetype into `destination_archetype`.
    ///
    /// The actual row movement is handled by `Archetype::move_row_to`.
    /// This helper only resolves the source and destination archetypes and returns
    /// the information needed for location updates.
    ///
    /// Returns `None` if:
    /// - The entity is not alive.
    /// - The entity has no stored location.
    /// - The source row is invalid.
    fn transfer_entity_row(
        &mut self,
        entity: Entity,
        destination_archetype: ArchetypeId,
    ) -> Option<(ArchetypeId, usize, RowMoveResult)> {
        if !self.is_alive(entity) {
            return None;
        }

        let source_location = self.location(entity)?;
        let source_id = source_location.archetype();
        let source_row = source_location.row();

        if source_id == destination_archetype {
            return Some((
                source_id,
                source_row,
                RowMoveResult {
                    destination_row: source_row,
                    swapped_entity: None,
                },
            ));
        }

        let source_index = source_id as usize;
        let destination_index = destination_archetype as usize;

        let result = if source_index < destination_index {
            let (left, right) = self.archetypes.split_at_mut(destination_index);
            let source = &mut left[source_index];
            let destination = &mut right[0];

            source.move_row_to(source_row, destination)
        } else {
            let (left, right) = self.archetypes.split_at_mut(source_index);
            let destination = &mut left[destination_index];
            let source = &mut right[0];

            source.move_row_to(source_row, destination)
        }?;

        Some((source_id, source_row, result))
    }

    fn build_columns_for_signature(&self, signature: &ArchetypeSignature) -> Vec<Column> {
        signature
            .component_ids()
            .iter()
            .map(|&component_id| {
                let info = self
                    .components
                    .info(component_id)
                    .unwrap_or_else(|| {
                        panic!("missing ComponentInfo for component ID {}", component_id)
                    })
                    .clone();

                Column::new(info)
            })
            .collect()
    }

    fn signature_with_added(
        &self,
        signature: &ArchetypeSignature,
        component_id: ComponentId,
    ) -> ArchetypeSignature {
        let mut ids = signature.component_ids().to_vec();

        if !ids.contains(&component_id) {
            ids.push(component_id);
        }

        ArchetypeSignature::new(ids)
    }

    /// Adds a component of type `T` to an entity.
    ///
    /// This moves the entity into a new archetype that includes `T` in its
    /// component signature.
    ///
    /// The operation:
    /// - Computes the destination archetype signature (old + `T`).
    /// - Finds or creates the destination archetype.
    /// - Transfers the entity row and all shared component data.
    /// - Appends the new component value into the destination-only column.
    /// - Updates all affected entity locations.
    ///
    /// If the entity already has a component of type `T`, the existing value
    /// is overwritten in-place.
    ///
    /// Returns `false` if:
    /// - The entity is not alive.
    /// - The component type has not been registered.
    ///
    /// # Invariants
    ///
    /// After this operation:
    /// - The entity exists in exactly one archetype.
    /// - All component columns in that archetype have the same length as `entities`.
    /// - The new component value is stored at the entity's row.
    pub fn add_component<T: 'static>(&mut self, entity: Entity, value: T) -> bool {
        if !self.is_alive(entity) {
            return false;
        }

        let component_id = match self.component_id::<T>() {
            Some(id) => id,
            None => return false,
        };

        let location = match self.location(entity) {
            Some(loc) => loc,
            None => return false,
        };

        let source_id = location.archetype();
        let source_row = location.row();

        let source_signature = self.archetypes[source_id as usize].signature().clone();

        // If the entity already has T, overwrite in place.
        if source_signature.contains(component_id) {
            let archetype = &mut self.archetypes[source_id as usize];
            let column = archetype.column_mut(component_id).unwrap();

            let slot = column.get_mut::<T>(source_row).unwrap();
            *slot = value;

            return true;
        }

        // Build the destination signature with the new component included.
        let destination_signature = self.signature_with_added(&source_signature, component_id);
        let destination_archetype = self.find_or_create_archetype(destination_signature);

        // Transfer the existing row structure.
        let (actual_source_archetype, old_source_row, move_result) =
            match self.transfer_entity_row(entity, destination_archetype) {
                Some(result) => result,
                None => return false,
            };

        debug_assert!(
            self.archetypes[actual_source_archetype as usize]
                .columns()
                .iter()
                .all(|c| c.len() == self.archetypes[actual_source_archetype as usize].len()),
            "source archetype not aligned after transfer"
        );

        // Immediately initialize the destination-only column for T.
        //
        // This restores full destination row alignment after the structural move.
        {
            let archetype = &mut self.archetypes[destination_archetype as usize];
            let column = archetype.column_mut(component_id).unwrap();

            column.push(value);

            debug_assert_eq!(column.len(), archetype.len());
        }

        // Fix the location of any entity that got swap-moved inside the source archetype.
        if let Some(swapped_entity) = move_result.swapped_entity {
            let swapped_index = swapped_entity.index as usize;
            self.entity_locations[swapped_index] =
                Some(EntityLocation::new(actual_source_archetype, old_source_row));
        }

        // Record the moved entity's new location.
        let entity_index = entity.index as usize;
        self.entity_locations[entity_index] = Some(EntityLocation::new(
            destination_archetype,
            move_result.destination_row,
        ));

        debug_assert!(
            self.archetypes[destination_archetype as usize]
                .columns()
                .iter()
                .all(|c| c.len() == self.archetypes[destination_archetype as usize].len()),
            "destination archetype not fully aligned after add_component"
        );

        true
    }

    /// Builds a new archetype signature by removing a component ID
    /// from an existing signature.
    ///
    /// This:
    /// - Copies all component IDs except `component_id`.
    /// - Re-normalizes the result (sorted + deduplicated).
    ///
    /// # Notes
    ///
    /// - If the component is not present, the signature is unchanged.
    /// - The returned signature is always canonical (sorted, unique).
    fn signature_with_removed(
        &self,
        signature: &ArchetypeSignature,
        component_id: ComponentId,
    ) -> ArchetypeSignature {
        // Filter out the component we want to remove.
        let ids = signature
            .component_ids()
            .iter()
            .copied()
            .filter(|&id| id != component_id)
            .collect();

        // Rebuild normalized signature (sort + dedup happens inside).
        ArchetypeSignature::new(ids)
    }

    /// Removes a component of type `T` from an entity.
    ///
    /// This moves the entity into a new archetype that no longer contains `T`.
    ///
    /// The operation:
    /// - Validates entity liveness.
    /// - Checks that the component type is registered.
    /// - Verifies the entity currently has the component.
    /// - Computes the destination signature (old - `T`).
    /// - Transfers the entity row and all remaining shared components.
    /// - Drops the removed component during source-row removal.
    /// - Updates all affected entity locations.
    ///
    /// Returns `false` if:
    /// - The entity is not alive.
    /// - The component type is not registered.
    /// - The entity does not have this component.
    ///
    /// # Invariants
    ///
    /// After this operation:
    /// - The entity exists in exactly one archetype.
    /// - The removed component is dropped.
    /// - All remaining component columns remain aligned.
    pub fn remove_component<T: 'static>(&mut self, entity: Entity) -> bool {
        if !self.is_alive(entity) {
            return false;
        }

        let component_id = match self.component_id::<T>() {
            Some(id) => id,
            None => return false,
        };

        let location = match self.location(entity) {
            Some(location) => location,
            None => return false,
        };

        let source_archetype = location.archetype();

        let source_signature = self.archetypes[source_archetype as usize]
            .signature()
            .clone();

        if !source_signature.contains(component_id) {
            return false;
        }

        let destination_signature = self.signature_with_removed(&source_signature, component_id);

        let destination_archetype = self.find_or_create_archetype(destination_signature);

        let (actual_source_archetype, old_source_row, move_result) =
            match self.transfer_entity_row(entity, destination_archetype) {
                Some(result) => result,
                None => return false,
            };

        // Fix the location of any entity that got swap-moved inside the source archetype.
        if let Some(swapped_entity) = move_result.swapped_entity {
            let swapped_index = swapped_entity.index as usize;
            self.entity_locations[swapped_index] =
                Some(EntityLocation::new(actual_source_archetype, old_source_row));
        }

        // Record the moved entity's new location.
        let entity_index = entity.index as usize;
        self.entity_locations[entity_index] = Some(EntityLocation::new(
            destination_archetype,
            move_result.destination_row,
        ));

        debug_assert!(
            self.archetypes[destination_archetype as usize]
                .columns()
                .iter()
                .all(|c| c.len() == self.archetypes[destination_archetype as usize].len()),
            "destination archetype not aligned after remove_component"
        );

        true
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
    fn creating_archetype_builds_matching_columns() {
        let mut world = World::new();

        world.register_component::<u32>();
        world.register_component::<f32>();

        let signature = ArchetypeSignature::new(vec![0, 1]);
        let archetype_id = world.find_or_create_archetype(signature.clone());

        let archetype = world.archetype(archetype_id).unwrap();

        assert_eq!(archetype.signature(), &signature);
        assert_eq!(archetype.columns().len(), 2);
        assert!(archetype.column(0).is_some());
        assert!(archetype.column(1).is_some());
    }

    #[test]
    fn add_component_creates_new_archetype() {
        let mut world = World::new();

        let e = world.spawn();
        let _ = world.register_component::<u32>();

        assert!(world.add_component(e, 42_u32));

        let loc = world.location(e).unwrap();
        let arch = world.archetype(loc.archetype()).unwrap();

        assert_eq!(arch.signature().component_ids().len(), 1);
    }

    #[test]
    fn add_component_stores_value() {
        let mut world = World::new();

        let e = world.spawn();
        let id = world.register_component::<u32>();

        assert!(world.add_component(e, 123_u32));

        let loc = world.location(e).unwrap();
        let arch = world.archetype(loc.archetype()).unwrap();

        let col = arch.column(id).unwrap();
        assert_eq!(col.get::<u32>(loc.row()), Some(&123));
    }

    #[test]
    fn add_component_overwrites_existing_value() {
        let mut world = World::new();

        let e = world.spawn();
        let id = world.register_component::<u32>();

        assert!(world.add_component(e, 10_u32));
        assert!(world.add_component(e, 20_u32));

        let loc = world.location(e).unwrap();
        let arch = world.archetype(loc.archetype()).unwrap();

        let col = arch.column(id).unwrap();
        assert_eq!(col.get::<u32>(loc.row()), Some(&20));
    }

    #[test]
    fn destroy_removes_component_rows() {
        let mut world = World::new();

        let e = world.spawn();
        let id = world.register_component::<u32>();

        assert!(world.add_component(e, 7_u32));

        let loc = world.location(e).unwrap();
        let arch_id = loc.archetype();

        assert!(world.destroy(e));

        let arch = world.archetype(arch_id).unwrap();
        let col = arch.column(id).unwrap();

        assert_eq!(arch.len(), 0);
        assert_eq!(col.len(), 0);
    }

    #[test]
    fn remove_component_moves_entity_to_smaller_signature() {
        let mut world = World::new();

        let e = world.spawn();
        world.register_component::<u32>();
        let f32_id = world.register_component::<f32>();

        assert!(world.add_component(e, 10_u32));
        assert!(world.add_component(e, 1.5_f32));

        assert!(world.remove_component::<u32>(e));

        let loc = world.location(e).unwrap();
        let arch = world.archetype(loc.archetype()).unwrap();

        assert_eq!(arch.signature().component_ids(), &[f32_id]);
        assert_eq!(
            arch.column(f32_id).unwrap().get::<f32>(loc.row()),
            Some(&1.5_f32)
        );
    }

    #[test]
    fn remove_component_returns_false_if_absent() {
        let mut world = World::new();

        let e = world.spawn();
        world.register_component::<u32>();

        assert!(!world.remove_component::<u32>(e));
    }

    #[test]
    fn remove_component_moves_back_to_empty_archetype() {
        let mut world = World::new();

        let e = world.spawn();
        world.register_component::<u32>();

        assert!(world.add_component(e, 99_u32));
        assert!(world.remove_component::<u32>(e));

        let loc = world.location(e).unwrap();
        assert_eq!(loc.archetype(), world.empty_archetype_id());
    }

    #[test]
    fn remove_component_updates_swapped_source_entity_location() {
        let mut world = World::new();

        world.register_component::<u32>();

        let e1 = world.spawn();
        let e2 = world.spawn();
        let e3 = world.spawn();

        assert!(world.add_component(e1, 1_u32));
        assert!(world.add_component(e2, 2_u32));
        assert!(world.add_component(e3, 3_u32));

        let source_archetype = world.location(e2).unwrap().archetype();

        assert!(world.remove_component::<u32>(e2));

        let e3_location = world.location(e3).unwrap();
        if e3_location.archetype() == source_archetype {
            assert_eq!(e3_location.row(), 1);
        }
    }

    #[test]
    fn add_component_updates_swapped_source_entity_location() {
        let mut world = World::new();

        world.register_component::<u32>();

        let e1 = world.spawn();
        let e2 = world.spawn();
        let e3 = world.spawn();

        let empty_id = world.empty_archetype_id();

        assert!(world.add_component(e2, 22_u32));

        let empty = world.archetype(empty_id).unwrap();
        assert_eq!(empty.len(), 2);
        assert!(empty.entities().contains(&e1));
        assert!(empty.entities().contains(&e3));

        let e3_location = world.location(e3).unwrap();
        assert_eq!(e3_location.archetype(), empty_id);
        assert_eq!(e3_location.row(), 1);
    }

    #[test]
    fn multiple_add_remove_keeps_alignment() {
        let mut world = World::new();

        world.register_component::<u32>();
        let e = world.spawn();

        for i in 0_u32..100 {
            assert!(world.add_component(e, i));
            assert!(world.remove_component::<u32>(e));
        }
    }
}
