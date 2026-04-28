use std::alloc::{self, Layout};
use std::any::TypeId;
use std::ptr::NonNull;

use crate::ecs::component::ComponentId;

/// Metadata describing a registered component type.
#[derive(Clone)]
pub struct ComponentInfo {
    /// Runtime ID for this component type.
    id: ComponentId,

    /// Rust `TypeId` for this component type.
    type_id: TypeId,

    /// Size and alignment information for this component type.
    layout: Layout,

    /// Function used to drop one initialized value of this component type.
    drop_fn: unsafe fn(*mut u8),

    /// Function used to move one initialized value from source storage
    /// to destination storage of the same component type.
    move_fn: unsafe fn(*mut u8, *mut u8),
}

impl ComponentInfo {
    /// Creates metadata for a component type.
    pub fn new<T: 'static>(id: ComponentId) -> Self {
        Self {
            id,
            type_id: TypeId::of::<T>(),
            layout: Layout::new::<T>(),
            drop_fn: drop_ptr::<T>,
            move_fn: move_ptr::<T>,
        }
    }

    /// Returns the runtime ID of this component type.
    pub fn id(&self) -> ComponentId {
        self.id
    }

    /// Returns the `TypeId` of this component type.
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Returns the memory layout of this component type.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Returns the function used to drop one stored component value.
    pub fn drop_fn(&self) -> unsafe fn(*mut u8) {
        self.drop_fn
    }

    /// Returns the size of one component value in bytes.
    pub fn size(&self) -> usize {
        self.layout.size()
    }

    /// Returns the alignment of this component type in bytes.
    pub fn align(&self) -> usize {
        self.layout.align()
    }

    /// Returns the function used to move one stored component value.
    pub fn move_fn(&self) -> unsafe fn(*mut u8, *mut u8) {
        self.move_fn
    }
}

/// Dense raw storage for component values of a single type.
///
/// A column stores only one component type, described by its `ComponentInfo`.
/// Values are tightly packed and indexed by row.
///
/// # Invariants
///
/// - `ptr` is allocated with alignment suitable for the component type.
/// - The first `len` elements are initialized.
/// - Elements in the range `len..capacity` are uninitialized.
/// - All stored values have the type described by `info`.
pub struct Column {
    info: ComponentInfo,
    ptr: NonNull<u8>,
    len: usize,
    capacity: usize,
}

impl Column {
    /// Creates an empty column for one component type.
    pub fn new(info: ComponentInfo) -> Self {
        Self {
            info,
            ptr: NonNull::dangling(),
            len: 0,
            capacity: 0,
        }
    }

    /// Returns metadata describing the component type stored in this column.
    pub fn info(&self) -> &ComponentInfo {
        &self.info
    }

    /// Returns the number of initialized component values in the column.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the column contains no values.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn assert_type<T: 'static>(&self) {
        assert_eq!(
            self.info.type_id(),
            TypeId::of::<T>(),
            "column type mismatch"
        );
    }

    fn layout_for_capacity(&self, capacity: usize) -> Layout {
        let size = self
            .info
            .size()
            .checked_mul(capacity)
            .expect("column capacity overflow");

        Layout::from_size_align(size, self.info.align()).expect("invalid column layout")
    }

    fn grow(&mut self) {
        // Zero-sized types have no bytes to store, so no allocation is needed.
        // Setting capacity to usize::MAX prevents push() from calling grow() again.
        if self.info.size() == 0 {
            self.capacity = usize::MAX;
            return;
        }

        let new_capacity = if self.capacity == 0 {
            4
        } else {
            self.capacity * 2
        };

        let new_layout = self.layout_for_capacity(new_capacity);

        unsafe {
            let new_ptr = if self.capacity == 0 {
                alloc::alloc(new_layout)
            } else {
                let old_layout = self.layout_for_capacity(self.capacity);
                alloc::realloc(self.ptr.as_ptr(), old_layout, new_layout.size())
            };

            self.ptr = NonNull::new(new_ptr).expect("allocation failed");
        }

        self.capacity = new_capacity;
    }

    fn element_ptr(&self, index: usize) -> *mut u8 {
        // Zero-sized types occupy no bytes, so all indices map to the same
        // dangling-but-aligned address. Reading or writing zero bytes through
        // a non-null aligned pointer is always valid in Rust.
        if self.info.size() == 0 {
            return self.ptr.as_ptr();
        }

        assert!(index < self.capacity);

        let offset = index
            .checked_mul(self.info.size())
            .expect("element offset overflow");

        unsafe { self.ptr.as_ptr().add(offset) }
    }

    /// Appends one component value to the column.
    pub fn push<T: 'static>(&mut self, value: T) {
        self.assert_type::<T>();

        if self.len == self.capacity {
            self.grow();
        }

        unsafe {
            self.element_ptr(self.len).cast::<T>().write(value);
        }

        self.len += 1;
    }

    /// Returns an immutable reference to the value at `index`.
    pub fn get<T: 'static>(&self, index: usize) -> Option<&T> {
        self.assert_type::<T>();

        if index >= self.len {
            return None;
        }

        unsafe { Some(&*self.element_ptr(index).cast::<T>()) }
    }

    /// Returns a mutable reference to the value at `index`.
    pub fn get_mut<T: 'static>(&mut self, index: usize) -> Option<&mut T> {
        self.assert_type::<T>();

        if index >= self.len {
            return None;
        }

        unsafe { Some(&mut *self.element_ptr(index).cast::<T>()) }
    }

    /// Removes and returns the value at `index` using swap-remove.
    ///
    /// This does not preserve order.
    pub fn swap_remove<T: 'static>(&mut self, index: usize) -> Option<T> {
        self.assert_type::<T>();

        if index >= self.len {
            return None;
        }

        let last_index = self.len - 1;

        unsafe {
            let removed = self.element_ptr(index).cast::<T>().read();

            if index != last_index {
                let last_value = self.element_ptr(last_index).cast::<T>().read();
                self.element_ptr(index).cast::<T>().write(last_value);
            }

            self.len -= 1;
            Some(removed)
        }
    }

    /// Returns `true` if this column stores the same component type as `other`.
    pub fn has_same_type(&self, other: &Column) -> bool {
        self.info.type_id() == other.info.type_id()
    }

    /// Returns the component ID stored in this column.
    pub fn component_id(&self) -> ComponentId {
        self.info.id()
    }

    fn reserve_one(&mut self) {
        if self.len == self.capacity {
            self.grow();
        }
    }

    /// Moves the element at `index` from this column into the end of `destination`.
    ///
    /// This removes the source element using swap-remove semantics and appends it
    /// to the destination column.
    ///
    /// Returns `false` if the index is out of bounds or the column types differ.
    pub fn move_element_to(&mut self, index: usize, destination: &mut Column) -> bool {
        if index >= self.len {
            return false;
        }

        if !self.has_same_type(destination) {
            return false;
        }

        destination.reserve_one();

        let last_index = self.len - 1;

        unsafe {
            let src_ptr = self.element_ptr(index);
            let dst_ptr = destination.element_ptr(destination.len);

            // Move the removed element into the destination.
            (self.info.move_fn())(src_ptr, dst_ptr);
            destination.len += 1;

            // If we removed a non-last row, move the last source element down
            // into the vacated slot.
            if index != last_index {
                let last_ptr = self.element_ptr(last_index);
                (self.info.move_fn())(last_ptr, src_ptr);
            }

            self.len -= 1;
        }

        true
    }

    /// Removes the element at `index` using swap-remove and drops it.
    ///
    /// Returns `false` if `index` is out of bounds.
    pub fn swap_remove_and_drop(&mut self, index: usize) -> bool {
        if index >= self.len {
            return false;
        }

        let last_index = self.len - 1;

        unsafe {
            let removed_ptr = self.element_ptr(index);

            if index != last_index {
                let last_ptr = self.element_ptr(last_index);

                // Drop the removed element first.
                (self.info.drop_fn())(removed_ptr);

                // Move the last element into the vacated slot.
                (self.info.move_fn())(last_ptr, removed_ptr);
            } else {
                (self.info.drop_fn())(removed_ptr);
            }

            self.len -= 1;
        }

        true
    }

    /// Moves the element at `index` into the end of `destination` without
    /// compacting or shrinking the source column.
    ///
    /// After this call:
    /// - The destination has one additional initialized element.
    /// - The source slot at `index` is now logically uninitialized.
    /// - The source column length is unchanged.
    ///
    /// This is intended for archetype-level row transfer, where compaction is
    /// handled later in a separate pass so all columns stay row-consistent.
    ///
    /// Returns `false` if:
    /// - `index` is out of bounds.
    /// - The columns store different component types.
    pub fn move_to_other_without_compacting(
        &mut self,
        index: usize,
        destination: &mut Column,
    ) -> bool {
        if index >= self.len {
            return false;
        }

        if !self.has_same_type(destination) {
            return false;
        }

        destination.reserve_one();

        unsafe {
            let src_ptr = self.element_ptr(index);
            let dst_ptr = destination.element_ptr(destination.len);

            // Move the initialized value into the destination.
            (self.info.move_fn())(src_ptr, dst_ptr);
            destination.len += 1;
        }

        true
    }

    /// Drops the element at `index` in place without compacting or shrinking
    /// the column.
    ///
    /// After this call:
    /// - The slot at `index` is logically uninitialized.
    /// - The column length is unchanged.
    ///
    /// The caller is responsible for either:
    /// - Overwriting the hole with the last element, then shrinking.
    /// - Shrinking if `index` was already the last row.
    ///
    /// Returns `false` if `index` is out of bounds.
    pub fn drop_in_place_at(&mut self, index: usize) -> bool {
        if index >= self.len {
            return false;
        }

        unsafe {
            let ptr = self.element_ptr(index);
            (self.info.drop_fn())(ptr);
        }

        true
    }

    /// Overwrites `target_index` with the current last element without changing
    /// the logical length.
    ///
    /// This is the column-level half of archetype swap-remove.
    /// It assumes the caller has already made `target_index` logically
    /// uninitialized by either:
    /// - Moving its value out.
    /// - Dropping it in place.
    ///
    /// If `target_index` is already the last row, this is a no-op.
    ///
    /// Returns `false` if `target_index` is out of bounds.
    pub fn overwrite_with_last(&mut self, target_index: usize) -> bool {
        if target_index >= self.len {
            return false;
        }

        let last_index = self.len - 1;

        if target_index == last_index {
            return true;
        }

        unsafe {
            let last_ptr = self.element_ptr(last_index);
            let target_ptr = self.element_ptr(target_index);

            // Move the last initialized value into the hole at target_index.
            (self.info.move_fn())(last_ptr, target_ptr);
        }

        true
    }

    /// Shrinks the logical length of the column by one.
    ///
    /// This should be called only after the caller has already handled the old
    /// last row correctly, either by:
    /// - Moving it into a hole.
    /// - Deciding it was the removed row.
    ///
    /// Returns `false` if the column is already empty.
    pub fn shrink_len_by_one(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }

        self.len -= 1;
        true
    }

    /// Moves the value at `index` out of the column without compacting or
    /// shrinking the column.
    ///
    /// After this call:
    /// - The slot at `index` is logically uninitialized.
    /// - The column length is unchanged.
    ///
    /// The caller is responsible for compacting or shrinking the column before it
    /// is used again.
    ///
    /// Returns `None` if `index` is out of bounds.
    pub(crate) fn take_without_compacting<T: 'static>(&mut self, index: usize) -> Option<T> {
        self.assert_type::<T>();

        if index >= self.len {
            return None;
        }

        unsafe { Some(self.element_ptr(index).cast::<T>().read()) }
    }
}

impl Drop for Column {
    fn drop(&mut self) {
        unsafe {
            for index in 0..self.len {
                let ptr = self.element_ptr(index);
                (self.info.drop_fn())(ptr);
            }

            if self.capacity != 0 && self.info.size() > 0 {
                let layout = self.layout_for_capacity(self.capacity);
                alloc::dealloc(self.ptr.as_ptr(), layout);
            }
        }
    }
}

/// Drops a value of type `T` stored at `ptr`.
///
/// # Safety
///
/// `ptr` must be valid, properly aligned for `T`, and point to an initialized
/// value of type `T`.
unsafe fn drop_ptr<T>(ptr: *mut u8) {
    unsafe {
        ptr.cast::<T>().drop_in_place();
    }
}

/// Moves a value of type `T` from `src` to `dst`
///
/// # Safety
///
/// - `src` must point to a valid initialized `T`.
/// - `dst` must be valid writable storage for `T`.
/// - `src` and `dst` must be properly aligned for `T`.
/// - `dst` must not currently hold an initialized `T`.
unsafe fn move_ptr<T>(src: *mut u8, dst: *mut u8) {
    let value = unsafe { src.cast::<T>().read() };
    unsafe {
        dst.cast::<T>().write(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Position {
        x: f32,
        y: f32,
    }

    #[test]
    fn push_and_get_component() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        column.push(Position { x: 1.0, y: 2.0 });
        column.push(Position { x: 3.0, y: 4.0 });

        assert_eq!(column.len(), 2);
        assert_eq!(
            column.get::<Position>(0),
            Some(&Position { x: 1.0, y: 2.0 })
        );
        assert_eq!(
            column.get::<Position>(1),
            Some(&Position { x: 3.0, y: 4.0 })
        );
    }

    #[test]
    fn get_returns_none_out_of_bounds() {
        let info = ComponentInfo::new::<Position>(0);
        let column = Column::new(info);

        assert!(column.get::<Position>(0).is_none());
    }

    #[test]
    fn get_mut_allows_modification() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        column.push(Position { x: 1.0, y: 2.0 });

        let pos = column.get_mut::<Position>(0).unwrap();
        pos.x = 10.0;

        assert_eq!(column.get::<Position>(0).unwrap().x, 10.0);
    }

    #[test]
    fn swap_remove_removes_value() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        column.push(Position { x: 1.0, y: 2.0 });
        column.push(Position { x: 3.0, y: 4.0 });
        column.push(Position { x: 5.0, y: 6.0 });

        let removed = column.swap_remove::<Position>(1);

        assert_eq!(removed, Some(Position { x: 3.0, y: 4.0 }));
        assert_eq!(column.len(), 2);
        assert_eq!(
            column.get::<Position>(1),
            Some(&Position { x: 5.0, y: 6.0 })
        );
    }

    #[test]
    fn swap_remove_returns_none_out_of_bounds() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        assert!(column.swap_remove::<Position>(0).is_none());
    }

    #[test]
    fn dropping_column_drop_stored_values() {
        use std::cell::RefCell;
        use std::rc::Rc;

        struct Droppy {
            counter: Rc<RefCell<usize>>,
        }

        impl Drop for Droppy {
            fn drop(&mut self) {
                *self.counter.borrow_mut() += 1;
            }
        }

        let counter = Rc::new(RefCell::new(0));

        {
            let info = ComponentInfo::new::<Droppy>(0);
            let mut column = Column::new(info);

            column.push(Droppy {
                counter: counter.clone(),
            });
            column.push(Droppy {
                counter: counter.clone(),
            });
        }

        assert_eq!(*counter.borrow(), 2);
    }

    #[test]
    #[should_panic(expected = "column type mismatch")]
    fn get_with_wrong_type_panics() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        column.push(Position { x: 1.0, y: 2.0 });

        let _ = column.get::<i32>(0);
    }

    #[test]
    #[should_panic(expected = "column type mismatch")]
    fn push_with_wrong_type_panics() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        column.push(123_i32);
    }

    #[test]
    fn move_element_to_moves_value_between_columns() {
        let mut source = Column::new(ComponentInfo::new::<u32>(0));
        let mut destination = Column::new(ComponentInfo::new::<u32>(0));

        source.push(10_u32);
        source.push(20_u32);
        source.push(30_u32);

        assert!(source.move_element_to(1, &mut destination));

        assert_eq!(source.len(), 2);
        assert_eq!(destination.len(), 1);

        assert_eq!(destination.get::<u32>(0), Some(&20_u32));
        assert!(source.get::<u32>(0) == Some(&10_u32));
        assert!(source.get::<u32>(1) == Some(&30_u32));
    }

    #[test]
    fn move_element_to_rejects_type_mismatch() {
        let mut source = Column::new(ComponentInfo::new::<u32>(0));
        let mut destination = Column::new(ComponentInfo::new::<f32>(1));

        source.push(10_u32);

        assert!(!source.move_element_to(0, &mut destination));
    }

    #[test]
    fn zst_column_push_increments_len() {
        struct Marker;

        let info = ComponentInfo::new::<Marker>(0);
        let mut column = Column::new(info);

        column.push(Marker);
        column.push(Marker);
        column.push(Marker);

        assert_eq!(column.len(), 3);
    }

    #[test]
    fn zst_column_calls_drop_for_each_value() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct DroppingMarker;

        impl Drop for DroppingMarker {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }

        DROP_COUNT.store(0, Ordering::Relaxed);

        {
            let info = ComponentInfo::new::<DroppingMarker>(0);
            let mut column = Column::new(info);
            column.push(DroppingMarker);
            column.push(DroppingMarker);
        } // column dropped here, should invoke DroppingMarker::drop twice

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 2);
    }
}
