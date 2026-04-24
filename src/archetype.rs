use crate::column::Column;
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
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
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

/// Result of moving one row out of an archetype.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RowMoveResult {
    /// The row appended in the destination archetype.
    pub destination_row: usize,

    /// The entity that was swap-moved into the source row, if any.
    pub swapped_entity: Option<Entity>,
}

/// Stores entities that share the same component signature.
///
/// All entities in an archetype have exactly the same set of component types.
/// The archetype stores:
/// - Its signature (which components it contains).
/// - A dense list of entities.
/// - One dense component column for each component type in the signature.
///
/// Entity order is not stable; removals use swap-remove for efficiency.
///
/// # Invariants
///
/// - `columns.len()` matches `signature.component_ids().len()`.
/// - Each column corresponds to the component ID at the same signature index.
/// - When entity and component rows are updated through the row-aware archetype/world
///   operations, component column row counts match `entities.len()`.
pub struct Archetype {
    /// The component signature shared by all entities in this archetype.
    signature: ArchetypeSignature,

    /// Dense storage of entities in this archetype.
    entities: Vec<Entity>,

    /// Dense storage for each component type in the archetype.
    ///
    /// Each column corresponds to a component ID in the signature at the same index.
    /// Row `i` in every column stores the component values for the entity at
    /// `entities[i]`.
    ///
    /// # Invariants
    ///
    /// - `columns.len()` matches `signature.component_ids().len()`.
    /// - All columns have the same number of rows as `entities`.
    /// - Column ordering matches the sorted order of component IDs in the signature.
    columns: Vec<Column>,
}

impl Archetype {
    /// Creates a new archetype with the given signature and columns.
    pub fn new(signature: ArchetypeSignature, columns: Vec<Column>) -> Self {
        debug_assert_eq!(signature.component_ids().len(), columns.len());

        Self {
            signature,
            entities: Vec::new(),
            columns,
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

    /// Returns the component columns stored in this archetype.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// Returns the component columns stored in this archetype.
    pub fn columns_mut(&mut self) -> &mut [Column] {
        &mut self.columns
    }

    /// Returns the index of the column for a component ID.
    pub fn column_index(&self, component_id: ComponentId) -> Option<usize> {
        self.signature
            .component_ids()
            .binary_search(&component_id)
            .ok()
    }

    /// Returns the column for a component ID.
    pub fn column(&self, component_id: ComponentId) -> Option<&Column> {
        let index = self.column_index(component_id)?;
        self.columns.get(index)
    }

    /// Returns the mutable column for a component ID.
    pub fn column_mut(&mut self, component_id: ComponentId) -> Option<&mut Column> {
        let index = self.column_index(component_id)?;
        self.columns.get_mut(index)
    }

    fn debug_assert_row_alignment(&self) {
        debug_assert!(
            self.columns
                .iter()
                .all(|column| column.len() == self.entities.len()),
            "archetype entity/column row counts are out of sync"
        );
    }

    /// Removes a full row from the archetype using swap-remove.
    ///
    /// This removes:
    /// - The entity at `row`.
    /// - The component value at `row` from every column.
    ///
    /// Component values are dropped as they are removed.
    ///
    /// Returns the removed entity.
    pub fn swap_remove_row_and_drop_components(&mut self, row: usize) -> Entity {
        self.debug_assert_row_alignment();

        for column in &mut self.columns {
            let removed = column.swap_remove_and_drop(row);
            debug_assert!(removed, "column row removal failed");
        }

        let removed_entity = self.entities.swap_remove(row);

        self.debug_assert_row_alignment();
        removed_entity
    }

    /// Moves one entity row from this archetype into `destination`.
    ///
    /// Shared components are moved into destination columns.
    /// Components not present in the destination are dropped.
    /// Source compaction is performed only after all row effects have been
    /// decided, so every source column uses the same `(source_row, last_row)` pair.
    ///
    /// This keeps source columns row-consistent during structural changes.
    ///
    /// # Notes
    ///
    /// - The destination entity row is appended immediately.
    /// - Shared destination columns are appended immediately.
    /// - If the destination has extra columns not present in the source
    ///   (for example during `add_component`), the caller must initialize those
    ///   destination-only columns immediately after this function returns.
    /// - Source row alignment is restored before returning.
    ///
    /// Returns `None` if `source_row` is out of bounds.
    pub fn move_row_to(
        &mut self,
        source_row: usize,
        destination: &mut Archetype,
    ) -> Option<RowMoveResult> {
        self.debug_assert_row_alignment();

        if source_row >= self.entities.len() {
            return None;
        }

        let removed_entity = self.entities[source_row];
        let last_row = self.entities.len() - 1;

        // Snapshot the source component IDs before mutating columns.
        let source_component_ids = self.signature.component_ids().to_vec();

        // Append the entity first so destination shared columns can append
        // into a valid new row.
        let destination_row = destination.push_entity(removed_entity);

        // Phase 1: remove the source-row value from each source column.
        //
        // - Shared component: move source_row into destination, leaving a hole.
        // - Removed component: drop source_row in place, leaving a hole.
        for component_id in &source_component_ids {
            let src_index = self
                .column_index(*component_id)
                .expect("source column missing for source signature component");

            if let Some(dst_index) = destination.column_index(*component_id) {
                let src_column = &mut self.columns[src_index];
                let dst_column = &mut destination.columns[dst_index];

                let moved = src_column.move_to_other_without_compacting(source_row, dst_column);
                debug_assert!(moved, "shared component move failed");
            } else {
                let src_column = &mut self.columns[src_index];

                let removed = src_column.drop_in_place_at(source_row);
                debug_assert!(removed, "removed component drop failed");
            }
        }

        // Phase 2: compact the source columns consistently.
        //
        // Every source column uses the same old `last_row`, so row alignment
        // across the archetype is preserved.
        if source_row != last_row {
            for component_id in &source_component_ids {
                let src_index = self
                    .column_index(*component_id)
                    .expect("source column missing during compaction");

                let src_column = &mut self.columns[src_index];

                let overwritten = src_column.overwrite_with_last(source_row);
                debug_assert!(overwritten, "source column overwrite failed");
            }
        }

        for component_id in &source_component_ids {
            let src_index = self
                .column_index(*component_id)
                .expect("source column missing during shrink");

            let src_column = &mut self.columns[src_index];

            let shrunk = src_column.shrink_len_by_one();
            debug_assert!(shrunk, "source column shrink failed");
        }

        // Remove the entity row using the same swap-remove policy.
        let removed = self.entities.swap_remove(source_row);
        debug_assert_eq!(removed, removed_entity);

        let swapped_entity = if source_row < self.entities.len() {
            Some(self.entities[source_row])
        } else {
            None
        };

        self.debug_assert_row_alignment();

        // Destination alignment is fully restored here only if it had no extra
        // destination-only columns. During `add_component`, the caller must
        // append the new component value immediately after this call.
        Some(RowMoveResult {
            destination_row,
            swapped_entity,
        })
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
        let mut archetype = Archetype::new(sig, vec![]);

        let e = Entity {
            index: 0,
            generation: 0,
        };
        let row = archetype.push_entity(e);

        assert_eq!(row, 0);
        assert_eq!(archetype.entities(), &[e]);
    }

    #[test]
    fn remove_entity_row_swap_removes() {
        let sig = ArchetypeSignature::new(vec![]);
        let mut archetype = Archetype::new(sig, vec![]);

        let e1 = Entity {
            index: 0,
            generation: 0,
        };
        let e2 = Entity {
            index: 1,
            generation: 0,
        };
        let e3 = Entity {
            index: 2,
            generation: 0,
        };

        archetype.push_entity(e1);
        archetype.push_entity(e2);
        archetype.push_entity(e3);

        let removed = archetype.remove_entity_row(1);

        assert_eq!(removed, e2);
        assert_eq!(archetype.len(), 2);
        assert!(archetype.entities().contains(&e1));
        assert!(archetype.entities().contains(&e3));
    }

    #[test]
    fn archetype_stores_columns_for_signature() {
        use crate::column::Column;
        use crate::column::ComponentInfo;

        let signature = ArchetypeSignature::new(vec![1, 3]);

        let columns = vec![
            Column::new(ComponentInfo::new::<u32>(1)),
            Column::new(ComponentInfo::new::<f32>(3)),
        ];

        let archetype = Archetype::new(signature.clone(), columns);

        assert_eq!(archetype.signature(), &signature);
        assert_eq!(archetype.columns().len(), 2);
    }

    #[test]
    fn column_lookup_finds_existing_component() {
        use crate::column::Column;
        use crate::column::ComponentInfo;

        let signature = ArchetypeSignature::new(vec![1, 3]);

        let columns = vec![
            Column::new(ComponentInfo::new::<u32>(1)),
            Column::new(ComponentInfo::new::<f32>(3)),
        ];

        let archetype = Archetype::new(signature, columns);

        assert_eq!(archetype.column_index(1), Some(0));
        assert_eq!(archetype.column_index(3), Some(1));
        assert_eq!(archetype.column_index(2), None);
    }

    #[test]
    fn swap_remove_row_and_drop_components_removes_entity_and_component_rows() {
        use crate::column::{Column, ComponentInfo};

        let signature = ArchetypeSignature::new(vec![1]);
        let mut archetype =
            Archetype::new(signature, vec![Column::new(ComponentInfo::new::<u32>(1))]);

        let e1 = Entity {
            index: 0,
            generation: 0,
        };
        let e2 = Entity {
            index: 1,
            generation: 0,
        };

        archetype.push_entity(e1);
        archetype.column_mut(1).unwrap().push(10_u32);

        archetype.push_entity(e2);
        archetype.column_mut(1).unwrap().push(20_u32);

        let removed = archetype.swap_remove_row_and_drop_components(0);

        assert_eq!(removed, e1);
        assert_eq!(archetype.len(), 1);
        assert_eq!(archetype.entities(), &[e2]);

        let column = archetype.column(1).unwrap();
        assert_eq!(column.len(), 1);
        assert_eq!(column.get::<u32>(0), Some(&20));
    }
}
