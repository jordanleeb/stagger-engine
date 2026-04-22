use crate::component::ComponentId;
use crate::entity::Entity;

/// Identifier for an archetype.
/// 
/// Archetypes are typically stored in a collection inside the world and
/// referenced by this ID.
pub type ArchetypeId = u32;

/// Identifies the set of component types stored in an archetype.
/// 
/// The component IDs are:
/// - Sorted.
/// - Unique.
/// 
/// This ensures that signatures can be compared for equality and used
/// reliably as keys when organizing archetypes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchetypeSignature {
    component_ids: Vec<ComponentId>,
}

impl ArchetypeSignature {
    /// Creates a new archetype signature.
    /// 
    /// The input component IDs are normalized by:
    /// - Sorting them.
    /// - Removing duplicates.
    /// 
    /// This guarantees a canonical representation for each unique set of components.
    pub fn new(mut component_ids: Vec<ComponentId>) -> Self {
        // Ensure stable ordering for comparisons and lookups.
        component_ids.sort_unstable();

        // Remove duplicate component IDs.
        component_ids.dedup();

        Self { component_ids }
    }

    /// Returns the component IDs in this signature.
    pub fn component_ids(&self) -> &[ComponentId] {
        &self.component_ids
    }

    /// Returns `true` if the signature includes the given component ID.
    /// 
    /// Uses binary search, so the component IDs must remain sorted.
    pub fn contains(&self, component_id: ComponentId) -> bool {
        self.component_ids.binary_search(&component_id).is_ok()
    }
}

/// Stores entities that share the same component signature.
/// 
/// All entities in an archetype have exactly the same set of component types.
/// The archetype stores:
/// - Its signature (which components it contains).
/// - A dense list of entities.
/// 
/// Entity order is not stable; removals use swap-remove for efficiency.
pub struct Archetype {
    /// The component signature shared by all entities in this archetype.
    signature: ArchetypeSignature,

    /// Dense storage of entities in this archetype.
    entities: Vec<Entity>,
}

impl Archetype {
    /// Creates a new archetype with the given signature.
    pub fn new(signature: ArchetypeSignature) -> Self {
        Self {
            signature,
            entities: Vec::new(),
        }
    }

    /// Returns the signature of this archetype.
    pub fn signature(&self) -> &ArchetypeSignature {
        &self.signature
    }

    /// Returns the number of entities in this archetype.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Returns `true` if the archetype contains no entities.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Returns a slice of entities stored in this archetype.
    /// 
    /// The order is not guaranteed to be stable across removals.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// Adds an entity to the archetype.
    /// 
    /// Returns the row index where the entity was inserted.
    pub fn push_entity(&mut self, entity: Entity) -> usize {
        let row = self.entities.len();
        self.entities.push(entity);
        row
    }

    /// Removes an entity at the given row index using swap-remove.
    /// 
    /// This:
    /// - Replaces the removed entity with the last entity.
    /// - Reduces the length by one.
    /// 
    /// Returns the removed entity.
    /// 
    /// Note: This operation does not preserve entity order.
    pub fn remove_entity_row(&mut self, row: usize) -> Entity {
        self.entities.swap_remove(row)
    }

    /// Returns `true` if the given entity is stored in this archetype.
    pub fn contains_entity(&self, entity: Entity) -> bool {
        self.entities.contains(&entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_sorts_component_ids() {
        let sig = ArchetypeSignature::new(vec![3, 1, 2]);
        assert_eq!(sig.component_ids(), &[1, 2, 3]);
    }

    #[test]
    fn signature_deduplicates_component_ids() {
        let sig = ArchetypeSignature::new(vec![3, 1, 2, 1, 3]);
        assert_eq!(sig.component_ids(), &[1, 2, 3]);
    }

    #[test]
    fn signature_contains_registered_component() {
        let sig = ArchetypeSignature::new(vec![1, 3, 5]);
        assert!(sig.contains(3));
        assert!(!sig.contains(2));
    }

    #[test]
    fn push_entity_returns_row_index() {
        let sig = ArchetypeSignature::new(vec![]);
        let mut archetype = Archetype::new(sig);

        let e = Entity { index: 0, generation: 0 };
        let row = archetype.push_entity(e);

        assert_eq!(row, 0);
        assert_eq!(archetype.entities(), &[e]);
    }

    #[test]
    fn remove_entity_row_swap_removes() {
        let sig = ArchetypeSignature::new(vec![]);
        let mut archetype = Archetype::new(sig);

        let e1 = Entity { index: 0, generation: 0 };
        let e2 = Entity { index: 1, generation: 0 };
        let e3 = Entity { index: 2, generation: 0 };

        archetype.push_entity(e1);
        archetype.push_entity(e2);
        archetype.push_entity(e3);

        let removed = archetype.remove_entity_row(1);

        assert_eq!(removed, e2);
        assert_eq!(archetype.len(), 2);
        assert!(archetype.entities().contains(&e1));
        assert!(archetype.entities().contains(&e3));
    }
}